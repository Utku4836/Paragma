//! The commands. Each one lives in its own file; this one parses the
//! command line and works out which project a command is about.

mod doctor;
mod find;
mod finish;
mod git;
mod git_screen;
pub mod hook;
mod ls;
mod mv;
mod open;
mod read;
mod rm;
mod run;
mod screen;
mod setup;
mod setup_screen;
mod skill;
mod target;
mod tree;
mod write;

use crate::project::{Home, Project};
use crate::state::links::Links;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "pg",
    version,
    about = "Paragma: project memory for coding agents (rules, decisions, errors and their fixes, notes)"
)]
struct Cli {
    /// The project to work in (default: the project linked to this folder)
    #[arg(long, short = 'p', global = true)]
    project: Option<String>,

    /// Most tokens of output
    #[arg(long, global = true, default_value_t = 1200)]
    budget: usize,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start or continue a project's work here and get its brief: goal, steps, last handoff, matching rules
    Open(open::Args),
    /// End this conversation's work: tick off its steps, save its handoff for the next one
    Finish(finish::Args),
    /// Search memory: a topic, a question, an error line, a file or a code name
    Find(find::Args),
    /// Print a note (id, path), one rule, or code: file#Function, file:10-20
    Read(read::Args),
    /// Write a note or folder/, a rule, decision or error (--rule, --decision, --error), or a secret (--secret)
    Write(Box<write::Args>),
    /// Run the tests or a build: what fails, what is new, flaky or fixed, and the known fix
    Run(run::Args),
    /// The map of the project's code: files and their symbols, with the notes about them marked
    Tree(tree::Args),
    /// Projects, or the folders and notes of a project
    Ls(ls::Args),
    /// Rename or move a note, a folder or a project
    #[command(alias = "rename")]
    Mv(mv::Args),
    /// Take a note, a rule, a folder or a project out of memory (kept in .archive/ unless --hard)
    Rm(rm::Args),
    /// Check the project's notes, code and indexes
    Doctor(doctor::Args),
    /// Connect Paragma to your coding agents (Claude Code, Codex, Antigravity, OpenCode)
    Setup(setup::Args),
    /// Paragma's instructions for agents: the core, or a skill (coding, projects, knowledge)
    Skill(skill::Args),
    /// Back the memory up to GitHub and keep it the same on every computer
    Git(git::Args),
    /// Answers an agent's hook (put in place by pg setup): the event as JSON on stdin
    #[command(hide = true)]
    Hook(hook::Args),
}

/// What every command needs.
pub struct Context {
    pub home: Home,
    /// The project the command is about, if any.
    pub project: Option<Project>,
    pub budget: usize,
    pub cwd: PathBuf,
}

/// An error with the exit code it should end the program with.
#[derive(Debug)]
pub struct Exit {
    pub code: u8,
    pub message: String,
}

impl std::fmt::Display for Exit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Exit {}

/// Exit codes: 0 ok, 1 nothing found, 2 usage, 3 not found, 7 internal.
pub fn fail(code: u8, message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Exit { code, message: message.into() })
}

pub const NOTHING_FOUND: u8 = 1;
pub const USAGE: u8 = 2;
pub const NOT_FOUND: u8 = 3;

pub fn run() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => match e.downcast_ref::<Exit>() {
            Some(x) if x.code == NOTHING_FOUND => {
                println!("{}", x.message);
                ExitCode::from(x.code)
            }
            Some(x) => {
                eprintln!("pg: {}", x.message);
                ExitCode::from(x.code)
            }
            None => {
                eprintln!("pg: {e:#}");
                ExitCode::from(7)
            }
        },
    }
}

fn dispatch(cli: Cli) -> Result<()> {
    crate::output::mark("parsed");
    crate::upgrade::run(&Home::locate());
    crate::output::mark("upgrade checked");
    if let Command::Hook(a) = cli.command {
        return hook::run(a);
    }
    let cx = context(cli.project.as_deref(), cli.budget)?;
    crate::output::mark("context");
    // what may change memory, or starts work that should see what other
    // computers sent: synced in the background afterwards
    let syncs = matches!(
        cli.command,
        Command::Open(_) | Command::Finish(_) | Command::Write(_) | Command::Mv(_) | Command::Rm(_)
    );
    let result = run_command(&cx, cli.command);
    if syncs {
        crate::sync::trigger(&cx.home);
    }
    result
}

fn run_command(cx: &Context, command: Command) -> Result<()> {
    match command {
        Command::Open(a) => open::run(cx, a),
        Command::Finish(a) => finish::run(cx, a),
        Command::Find(a) => find::run(cx, a),
        Command::Read(a) => read::run(cx, a),
        Command::Write(a) => write::run(cx, *a),
        Command::Run(a) => run::run(cx, a),
        Command::Tree(a) => tree::run(cx, a),
        Command::Ls(a) => ls::run(cx, a),
        Command::Mv(a) => mv::run(cx, a),
        Command::Rm(a) => rm::run(cx, a),
        Command::Doctor(a) => doctor::run(cx, a),
        Command::Setup(a) => setup::run(cx, a),
        Command::Skill(a) => skill::run(cx, a),
        Command::Git(a) => git::run(cx, a),
        Command::Hook(_) => unreachable!("answered above"),
    }
}

/// The project: --project, else $PARAGMA_PROJECT, else the one linked to
/// this folder.
fn context(flag: Option<&str>, budget: usize) -> Result<Context> {
    let home = Home::locate();
    let cwd = std::env::current_dir()?;
    let env = std::env::var("PARAGMA_PROJECT").ok().filter(|s| !s.is_empty());
    let name = match flag.map(str::to_string).or(env) {
        Some(n) => Some(n),
        None => Links::load(&home).projects_of(&cwd).into_iter().find(|p| home.project(p).is_some()),
    };
    let project = match name {
        Some(n) => {
            Some(home.project(&n).ok_or_else(|| fail(NOT_FOUND, format!("no project {n:?} (projects: pg ls)")))?)
        }
        None => None,
    };
    Ok(Context { home, project, budget, cwd })
}

/// Masks the secrets in text an agent hands over (record/secrets.rs),
/// collecting what kinds were found.
pub fn masked(text: &mut String, found: &mut Vec<&'static str>) {
    let (clean, kinds) = crate::record::secrets::mask(text);
    *text = clean;
    found.extend(kinds);
}

/// Masks every given text; None when there were no secrets.
pub fn mask_all<'a>(texts: impl IntoIterator<Item = &'a mut String>) -> Option<String> {
    let mut found = Vec::new();
    for t in texts {
        masked(t, &mut found);
    }
    crate::record::secrets::warning(&found)
}

impl Context {
    /// The command's project, or an error saying how to pick one.
    pub fn need_project(&self) -> Result<&Project> {
        self.project.as_ref().ok_or_else(|| fail(USAGE, "no project here: pass --project <name> (projects: pg ls)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_values_may_start_with_a_dash() {
        let parsed = |args: &[&str]| Cli::try_parse_from(args).map(|_| ()).map_err(|e| e.to_string());
        let ok = [
            &[
                "pg",
                "write",
                "--error",
                "--locked failed",
                "--cause",
                "--ignore-user-config loads no hooks",
                "--fix",
                "-c hooks={...}",
            ][..],
            &[
                "pg",
                "write",
                "--decision",
                "-O2 for release",
                "--why",
                "--release alone was slow",
                "--alternatives",
                "-O3",
            ],
            &["pg", "write", "--rule", "--force is never used", "--group", "Git"],
            &["pg", "open", "x", "--goal", "--json output", "--step", "- first", "--decision", "--no-default-features"],
            &["pg", "finish", "done", "--left", "--check in CI"],
        ];
        for a in ok {
            assert_eq!(parsed(a), Ok(()), "{a:?}");
        }
        // flags after a text value are still flags
        assert_eq!(parsed(&["pg", "write", "--error", "e", "--cause", "c", "--files", "a.rs"]), Ok(()));
        assert!(parsed(&["pg", "write", "--error", "e", "--cause", "c", "--fix"]).is_err());
    }
}
