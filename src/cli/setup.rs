//! pg setup: connect Paragma to the coding agents on this computer, for
//! every conversation (the agent's global instructions file) or for the
//! conversations in one folder (the instructions file there). Re-running
//! replaces Paragma's section; the rest of the file is kept.
//!
//! ```text
//! pg setup                          a screen to choose on, in a terminal
//! pg setup codex --global           every Codex conversation
//! pg setup claude --dir .           Claude Code conversations in this folder
//! pg setup all --global             every agent found
//! pg setup --list
//! ```

use super::setup_screen as screen;
use super::{Context, USAGE, fail};
use crate::agents::{self, Agent};
use crate::project::Home;
use crate::skills;
use crate::terminal;
use anyhow::Result;
use std::io::IsTerminal;
use std::path::PathBuf;

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
    for agent in &chosen {
        let path = match &folder {
            Some(f) => f.join(agent.folder_file),
            None => agent.global.clone(),
        };
        println!("{}", connect(&cx.home, &style, agent, &path)?);
    }
    Ok(())
}

/// Writes the section for an agent into `path`; the line saying how it went.
pub(super) fn connect(home: &Home, style: &Style, agent: &Agent, path: &std::path::Path) -> Result<String> {
    let what = if skills::install(path)? { "written" } else { "already up to date" };
    crate::upgrade::remember_setup(home, path);
    Ok(format!("  {} {:<12} {}  {}", style.ok("✓"), agent.name, style.dim(&agents::short(path)), style.dim(what)))
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
