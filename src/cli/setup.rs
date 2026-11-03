//! pg setup: connect Paragma to the coding agents on this computer, for
//! every conversation (the agent's global instructions file) or for the
//! conversations in one folder (the instructions file there). Re-running
//! replaces Paragma's section; the rest of the file is kept.
//!
//! With hooks (Claude Code and Codex), the agent also runs Paragma by
//! itself: the brief when a conversation starts, the known fix when a test
//! fails, a reminder to hand off (see cli/hook.rs, agents/hooks.rs).
//!
//! ```text
//! pg setup                          a screen to choose on, in a terminal
//! pg setup codex --global           every Codex conversation
//! pg setup claude --dir .           Claude Code conversations in this folder
//! pg setup claude --global --hooks  and Paragma's hooks
//! pg setup claude --global --no-hooks   takes the hooks out again
//! pg setup all --global             every agent found
//! pg setup --list
//! ```

use super::setup_screen as screen;
use super::{Context, USAGE, fail};
use crate::agents::{self, Agent, hooks};
use crate::project::Home;
use crate::skills;
use crate::terminal;
use anyhow::Result;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub struct Args {
    /// claude, codex, antigravity, opencode, or all (the ones found)
    agent: Option<String>,

    /// Every conversation of the agent (its global instructions file)
    #[arg(long, conflicts_with = "dir")]
    global: bool,

    /// Conversations in this folder (the instructions file there)
    #[arg(long, value_name = "FOLDER")]
    dir: Option<PathBuf>,

    /// Also Paragma's hooks (Claude Code and Codex): the agent runs Paragma by itself
    #[arg(long, conflicts_with = "no_hooks")]
    hooks: bool,

    /// Take Paragma's hooks out (the instructions stay)
    #[arg(long)]
    no_hooks: bool,

    /// The agents, where they read instructions, and whether Paragma is set up there
    #[arg(long)]
    list: bool,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    skills::write_copies(&cx.home)?;
    let style = Style::new();
    if a.list {
        list(&style);
        return Ok(());
    }
    let Some(id) = &a.agent else {
        if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() && screen::run(cx, &style)? {
            return Ok(());
        }
        println!("pg setup chooses on a screen in a terminal; in a script: pg setup <agent> --global | --dir <folder>");
        list(&style);
        return Ok(());
    };
    let chosen: Vec<Agent> = if id == "all" {
        agents::known().into_iter().filter(|x| x.installed).collect()
    } else {
        let names: Vec<&str> = agents::known().iter().map(|x| x.id).collect();
        vec![
            agents::find(id)
                .ok_or_else(|| fail(USAGE, format!("unknown agent {id:?} (agents: {}, all)", names.join(", "))))?,
        ]
    };
    let folder = match (&a.dir, a.global) {
        (Some(d), false) => Some(std::path::absolute(d)?),
        (None, true) => None,
        _ => {
            return Err(fail(
                USAGE,
                "say where: --global (every conversation) or --dir <folder> (conversations there)",
            ));
        }
    };
    if a.hooks || a.no_hooks {
        let without: Vec<&str> = chosen.iter().filter(|x| !hooks::AGENTS.contains(&x.id)).map(|x| x.name).collect();
        if id != "all" && !without.is_empty() {
            return Err(fail(USAGE, format!("hooks are for Claude Code and Codex only, for now (not {})", without[0])));
        }
    }
    for agent in chosen.iter().filter(|x| !(a.hooks || a.no_hooks) || hooks::AGENTS.contains(&x.id)) {
        if a.no_hooks {
            let path = hooks::file(agent.id, folder.as_deref());
            let what = match hooks::remove(&path)? {
                hooks::Change::Written => "hooks taken out",
                hooks::Change::Unchanged => "no hooks of Paragma there",
            };
            println!(
                "  {} {:<12} {}  {}",
                style.ok("✓"),
                agent.name,
                style.dim(&agents::short(&path)),
                style.dim(what)
            );
            continue;
        }
        let path = match &folder {
            Some(f) => f.join(agent.folder_file),
            None => agent.global.clone(),
        };
        let with_hooks = a.hooks.then(|| hooks::file(agent.id, folder.as_deref()));
        for line in connect(&cx.home, &style, agent, &path, with_hooks.as_deref())? {
            println!("{line}");
        }
    }
    Ok(())
}

/// Writes the section for an agent into `path`, and its hooks into
/// `hooks_file` when given; the lines saying how it went.
pub(super) fn connect(
    home: &Home,
    style: &Style,
    agent: &Agent,
    path: &Path,
    hooks_file: Option<&Path>,
) -> Result<Vec<String>> {
    let what = if skills::install(path)? { "written" } else { "already up to date" };
    crate::upgrade::remember_setup(home, path);
    let line = |p: &Path, what: &str| {
        format!("  {} {:<12} {}  {}", style.ok("✓"), agent.name, style.dim(&agents::short(p)), style.dim(what))
    };
    let mut lines = vec![line(path, what)];
    if let Some(file) = hooks_file {
        let exe = std::env::current_exe()?;
        let what = match hooks::install(agent.id, file, &exe)? {
            hooks::Change::Written => "hooks written",
            hooks::Change::Unchanged => "hooks already up to date",
        };
        crate::upgrade::remember_hooks(home, agent.id, file);
        lines.push(line(file, what));
    }
    Ok(lines)
}

fn list(style: &Style) {
    for agent in agents::known() {
        let mark = if agent.installed { style.accent("●") } else { style.dim("○") };
        let state = if skills::installed(&agent.global) {
            style.ok("set up")
        } else if agent.installed {
            "not set up".to_string()
        } else {
            style.dim("not found")
        };
        println!(
            "  {mark} {:<12} {:<11} {}  {}",
            agent.name,
            agent.id,
            style.dim(&format!("global: {}  folder: {}", agents::short(&agent.global), agent.folder_file)),
            state
        );
    }
}

/// Paragma's colours, when writing to a terminal (and NO_COLOR is not set).
pub struct Style {
    on: bool,
}

impl Style {
    fn new() -> Style {
        let on =
            std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() && terminal::enable_colours();
        Style { on }
    }

    /// No colours (tests).
    #[cfg(test)]
    pub(super) fn plain() -> Style {
        Style { on: false }
    }

    pub(super) fn paint(&self, code: &str, s: &str) -> String {
        if self.on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }

    /// Violet, the colour of Paragma.
    pub(super) fn accent(&self, s: &str) -> String {
        self.paint("1;38;2;139;92;246", s)
    }

    /// The darker violet of the banner's shadow.
    pub(super) fn shadow(&self, s: &str) -> String {
        self.paint("38;2;76;52;140", s)
    }

    pub(super) fn ok(&self, s: &str) -> String {
        self.paint("38;2;120;200;140", s)
    }

    pub(super) fn bad(&self, s: &str) -> String {
        self.paint("38;2;230;100;100", s)
    }

    pub(super) fn dim(&self, s: &str) -> String {
        self.paint("38;2;130;130;130", s)
    }

    pub(super) fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }
}
