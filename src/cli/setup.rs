//! pg setup: connect Paragma to the coding agents on this computer, for
//! every conversation (the agent's global instructions file) or for the
//! conversations in one folder (the instructions file there). Re-running
//! replaces Paragma's section; the rest of the file is kept.
//!
//! ```text
//! pg setup                          asks, in a terminal
//! pg setup codex --global           every Codex conversation
//! pg setup claude --dir .           Claude Code conversations in this folder
//! pg setup all --global             every agent found
//! pg setup --list
//! ```

use super::{Context, USAGE, fail};
use crate::agents::{self, Agent};
use crate::skills;
use anyhow::Result;
use std::io::{BufRead, IsTerminal, Write};
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
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            println!(
                "pg setup asks its questions in a terminal; in a script: pg setup <agent> --global | --dir <folder>"
            );
            list(&style);
            return Ok(());
        }
        return ask(cx, &style);
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
            return Err(fail(USAGE, "say where: --global (every conversation) or --dir <folder> (conversations there)"));
        }
    };
    connect(&style, &chosen, folder.as_deref())
}

/// Writes the section for each agent, and says how it went.
fn connect(style: &Style, chosen: &[Agent], folder: Option<&std::path::Path>) -> Result<()> {
    for agent in chosen {
        let path = match folder {
            Some(f) => f.join(agent.folder_file),
            None => agent.global.clone(),
        };
        let what = if skills::install(&path)? { "written" } else { "already up to date" };
        println!("  {} {:<12} {}  {}", style.ok("✓"), agent.name, style.dim(&agents::short(&path)), style.dim(what));
    }
    Ok(())
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

/// The questions, in a terminal.
fn ask(cx: &Context, style: &Style) -> Result<()> {
    let all = agents::known();
    println!();
    println!("  {} {}", style.accent("◆ Paragma"), style.dim("memory for coding agents"));
    println!();
    println!("  {}", style.bold("Agents on this computer"));
    for (i, agent) in all.iter().enumerate() {
        let mark = if agent.installed { style.accent("●") } else { style.dim("○") };
        let place = if agent.installed { agents::short(&agent.global) } else { "not found".to_string() };
        let set = if skills::installed(&agent.global) { format!("  {}", style.ok("set up")) } else { String::new() };
        println!("   {}  {mark} {:<12} {}{set}", style.dim(&(i + 1).to_string()), agent.name, style.dim(&place));
    }
    let found: Vec<usize> = all.iter().enumerate().filter(|(_, a)| a.installed).map(|(i, _)| i + 1).collect();
    println!();
    let default = found.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" ");
    let answer = prompt(
        style,
        &format!(
            "Which agents? numbers, Enter for {}",
            if default.is_empty() { "none".into() } else { default.clone() }
        ),
    )?;
    let picked: Vec<usize> = if answer.trim().is_empty() {
        found
    } else {
        answer
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|n| n.parse().ok())
            .filter(|n| (1..=all.len()).contains(n))
            .collect()
    };
    if picked.is_empty() {
        println!("  {}", style.dim("nothing to set up"));
        return Ok(());
    }
    println!();
    println!("  {}", style.bold("For which conversations?"));
    println!("   {}  every conversation {}", style.dim("1"), style.dim("(their global instructions)"));
    println!("   {}  conversations in this folder {}", style.dim("2"), style.dim(&cx.cwd.to_string_lossy()));
    let scope = prompt(style, "Enter for 1")?;
    let folder = (scope.trim() == "2").then(|| cx.cwd.clone());
    println!();
    let chosen: Vec<Agent> =
        all.into_iter().enumerate().filter(|(i, _)| picked.contains(&(i + 1))).map(|(_, a)| a).collect();
    connect(style, &chosen, folder.as_deref())?;
    println!();
    println!(
        "  {} In a conversation, ask for work: the agent opens a project with {}.",
        style.accent("Ready."),
        style.bold("pg open")
    );
    println!("  {}", style.dim("Skills: pg skill coding | projects | knowledge"));
    println!();
    Ok(())
}

fn prompt(style: &Style, question: &str) -> Result<String> {
    print!("  {} {} ", style.dim(question), style.accent("›"));
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line)
}

/// Colours when writing to a terminal (and NO_COLOR is not set).
struct Style {
    on: bool,
}

impl Style {
    fn new() -> Style {
        let on = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() && enable_ansi();
        Style { on }
    }

    fn paint(&self, code: &str, s: &str) -> String {
        if self.on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }

    fn accent(&self, s: &str) -> String {
        self.paint("1;38;5;141", s)
    }

    fn ok(&self, s: &str) -> String {
        self.paint("38;5;114", s)
    }

    fn dim(&self, s: &str) -> String {
        self.paint("38;5;245", s)
    }

    fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }
}

/// Windows consoles show colours once asked to.
#[cfg(windows)]
fn enable_ansi() -> bool {
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleMode,
    };
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode = 0;
        GetConsoleMode(h, &mut mode) != 0 && SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

#[cfg(not(windows))]
fn enable_ansi() -> bool {
    true
}
