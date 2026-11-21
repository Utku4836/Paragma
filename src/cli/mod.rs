//! The commands. Each one lives in its own file; this one parses the
//! command line and works out which project a command is about.

mod act;
mod ask;
mod batch;
mod doctor;
mod find;
mod finish;
mod git;
mod git_screen;
pub mod hook;
mod link;
mod ls;
mod mcp;
mod mv;
mod open;
mod perceive;
mod read;
mod ripple;
mod rm;
mod run;
mod screen;
mod setup;
mod setup_screen;
mod skill;
mod sleep;
mod steps;
mod target;
mod tell;
mod tree;
mod undo;
mod write;

use crate::project::{Home, Project};
use crate::state::links::Links;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

/// pg --help: the commands by what they do. Looking changes nothing;
/// changing memory is write, mv, rm and undo.
const HELP: &str = "{about}

{usage-heading} {usage}

Work:
  open    Start or continue a project's work here and get its brief: goal, steps, last handoff, matching rules
  finish  End this conversation's work: tick off its steps, save its handoff for the next one
  steps   The project's steps with their numbers; --take, --defer, --drop, --edit one
  ask     A question that waits on the user; pg ask 2 \"...\" answers it; alone: the open questions

Look (they change nothing):
  find    Search memory: a topic, a question, an error line, a file or a code name
  read    Print a note, a rule, a record, code (file#Function, file:10-20) or part of a document (book.xlsx#Sheet1!A1:D9)
  ls      Projects, a project's notes, its code (a code folder or file), to-dos (--todo), due dates (--due)

Change memory:
  write   A note, a rule, decision or error, a secret, a file kept with a note (--file), a due date, a table row
  mv      Rename or move a note, a folder or a project
  rm      Take a note, a rule, a folder or a project out of memory (kept in .archive/ unless --hard)
  undo    What pg did by itself, listed, or one taken back

Code:
  run     Run the tests or a build: what fails, what is new, flaky or fixed, and the known fix
  ripple  What a change reaches: callers it left as they were, the tests to run

Together:
  tell    A short note for the other conversations on this project
  batch   Several pg commands in one call, as JSON

For people:
  setup   Connect Paragma to your coding agents
  git     Keep the memory on GitHub, the same on every computer
  link    The folders a project's code is in
  doctor  Check the project's notes, code and indexes (--experts: what pg said and did)
  skill   Paragma's instructions for agents
  mcp     Paragma as an MCP server over stdio

{options-heading}
{options}
";

#[derive(Parser)]
#[command(
    name = "pg",
    bin_name = "pg",
    version,
    about = "Paragma: project memory for coding agents (rules, decisions, errors and their fixes, notes)",
    help_template = HELP
)]
struct Cli {
    /// The project to work in (default: the project linked to this folder)
    #[arg(long, short = 'p', global = true)]
    project: Option<String>,

    /// Most tokens of output
    #[arg(long, global = true, default_value_t = 1200)]
    budget: usize,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start or continue a project's work here and get its brief: goal, steps, last handoff, matching rules
    Open(open::Args),
    /// End this conversation's work: tick off its steps, save its handoff for the next one
    Finish(finish::Args),
    /// The project's steps with their numbers (as pg finish --done takes them); --drop or --edit one
    Steps(steps::Args),
    /// Keep a question that waits on the user (the brief lists the open ones); alone: the open questions
    Ask(ask::AskArgs),
    /// The user's answer to a question kept with pg ask (now: pg ask <number> "..."): by its number or words only it has
    #[command(hide = true)]
    Answer(ask::AnswerArgs),
    /// Search memory: a topic, a question, an error line, a file or a code name
    Find(find::Args),
    /// Print a note (id, path), one rule, or code: file#Function, file:10-20
    Read(read::Args),
    /// Write a note or folder/, a rule, decision or error (--rule, --decision, --error), or a secret (--secret)
    Write(Box<write::Args>),
    /// Run the tests or a build: what fails, what is new, flaky or fixed, and the known fix
    Run(run::Args),
    /// The map of the project's code (now: pg ls --code, pg ls <code folder or file>)
    #[command(hide = true)]
    Tree(tree::Args),
    /// What a change reaches: callers it left as they were, the tests to run (or what reaches one function)
    Ripple(ripple::Args),
    /// A short note for the other conversations on this project (each sees it once, for a day)
    Tell(tell::Args),
    /// Several pg commands in one call, as JSON: those that only look run side by side, each answers on its own
    Batch(batch::Args),
    /// What pg did by itself (a handoff, a draft record, steps closed): listed, or one taken back
    Undo(undo::Args),
    /// Projects, a project's folders and notes, its code (a code folder or file, --code), to-dos (--todo), due dates (--due)
    Ls(ls::Args),
    /// The folders a project's code is in: list, link one (default: here), --replace the others, --remove one
    Link(link::Args),
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
    /// Paragma as an MCP server over stdio, for clients without hooks: claude mcp add paragma -- pg mcp
    Mcp(mcp::Args),
    /// Answers an agent's hook (put in place by pg setup): the event as JSON on stdin
    #[command(hide = true)]
    Hook(hook::Args),
    /// The work done after pg finish, in the background (what may be outdated, what could be tidier)
    #[command(hide = true)]
    Sleep(sleep::Args),
}

/// The command line with its command.
struct Parsed {
    project: Option<String>,
    budget: usize,
    command: Command,
}

/// What pg did by itself, said for people (the interface's activity).
pub fn undo_describe(e: &crate::state::journal::Entry) -> String {
    undo::describe(e)
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
            None if crate::db::denied(&e) => {
                eprintln!(
                    "pg: {e:#}
{}",
                    not_let_in(&Home::locate())
                );
                ExitCode::from(7)
            }
            None if crate::db::broken(&e) => {
                eprintln!(
                    "pg: {e:#}
a file of pg's own is damaged: pg doctor finds it and makes it again"
                );
                ExitCode::from(7)
            }
            None => {
                eprintln!("pg: {e:#}");
                ExitCode::from(7)
            }
        },
    }
}

/// What to do when pg may not read or write a file it needs.
fn not_let_in(home: &Home) -> String {
    format!(
        "pg was not let in. It keeps memory in {} and writes there on most commands: \
         an agent's sandbox may keep it out (allow writing there, or run pg outside the sandbox), \
         or set PARAGMA_HOME to a folder pg may write. A file kept read-only does the same",
        home.root.display()
    )
}

fn dispatch(cli: Cli) -> Result<()> {
    crate::output::mark("parsed");
    // pg alone: the interface for people, in a terminal; else the commands
    let Some(command) = cli.command else {
        use std::io::IsTerminal;
        if std::io::stdout().is_terminal() && std::io::stdin().is_terminal() {
            let home = Home::locate();
            crate::upgrade::run(&home);
            let cx = context(cli.project.as_deref(), cli.budget)?;
            return crate::ui::run(home, cx.project);
        }
        use clap::CommandFactory;
        Cli::command().print_help()?;
        return Ok(());
    };
    let cli = Parsed { project: cli.project, budget: cli.budget, command };
    let home = Home::locate();
    crate::upgrade::run(&home);
    crate::code::files::keep_listings_in(home.projects_dir().join(".pg").join("listings"));
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
    ) || matches!(&cli.command, Command::Steps(a) if a.changes())
        || matches!(&cli.command, Command::Answer(_) | Command::Tell(_))
        || matches!(&cli.command, Command::Ask(a) if a.asks())
        || matches!(&cli.command, Command::Undo(a) if a.changes());
    // started before too: a command that stops early (its output closed)
    // still syncs; the sync waits for a quiet moment anyway
    if syncs {
        crate::sync::trigger(&cx.home);
    }
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
        Command::Steps(a) => steps::run(cx, a),
        Command::Ask(a) => ask::ask(cx, a),
        Command::Answer(a) => ask::answer(cx, a),
        Command::Find(a) => find::run(cx, a),
        Command::Read(a) => read::run(cx, a),
        Command::Write(a) => write::run(cx, *a),
        Command::Run(a) => run::run(cx, a),
        Command::Tree(a) => tree::run(cx, a),
        Command::Ripple(a) => ripple::run(cx, a),
        Command::Ls(a) => ls::run(cx, a),
        Command::Link(a) => link::run(cx, a),
        Command::Mv(a) => mv::run(cx, a),
        Command::Rm(a) => rm::run(cx, a),
        Command::Sleep(a) => sleep::run(cx, a),
        Command::Tell(a) => tell::run(cx, a),
        Command::Batch(a) => batch::run(cx, a),
        Command::Undo(a) => undo::run(cx, a),
        Command::Mcp(a) => mcp::run(cx, a),
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
        None => Links::load(&home)
            .projects_of(&cwd)
            .into_iter()
            .find(|p| home.project(p).is_some())
            .or_else(|| memory_folder_project(&home, &cwd)),
    };
    let project = match name {
        Some(n) => {
            Some(home.project(&n).ok_or_else(|| fail(NOT_FOUND, format!("no project {n:?} (projects: pg ls)")))?)
        }
        None => None,
    };
    Ok(Context { home, project, budget, cwd })
}

/// In a project's own folder in memory (~/.paragma/projects/shop, or a
/// folder in it): that project.
fn memory_folder_project(home: &Home, cwd: &std::path::Path) -> Option<String> {
    let projects = home.projects_dir();
    let rest = match cwd.strip_prefix(&projects) {
        Ok(r) => r.to_path_buf(),
        // the same folder written another way (case, a link)
        Err(_) => {
            let (c, p) = (std::fs::canonicalize(cwd).ok()?, std::fs::canonicalize(&projects).ok()?);
            let (cl, pl) = (c.to_string_lossy().to_lowercase(), p.to_string_lossy().to_lowercase());
            if cfg!(windows) && cl.starts_with(&pl) {
                std::path::PathBuf::from(
                    &c.to_string_lossy()[p.to_string_lossy().len()..].trim_start_matches(['\\', '/']),
                )
            } else {
                c.strip_prefix(&p).ok()?.to_path_buf()
            }
        }
    };
    let name = rest.components().next()?.as_os_str().to_str()?.to_string();
    home.project(&name).map(|p| p.name)
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
    /// The code folders linked to a project.
    pub fn code_dirs(&self, project: &Project) -> Vec<PathBuf> {
        Links::load(&self.home)
            .folders_of(&project.name)
            .into_iter()
            .map(PathBuf::from)
            .filter(|d| d.is_dir())
            .collect()
    }

    /// The command's project, or an error saying how to pick one.
    pub fn need_project(&self) -> Result<&Project> {
        self.project.as_ref().ok_or_else(|| fail(USAGE, "no project here: pass --project <name> (projects: pg ls)"))
    }
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/cli/mod.rs"]
mod tests;
