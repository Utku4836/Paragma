//! pg git: the memory backed up to GitHub and kept the same on every
//! computer (see sync/).
//!
//! ```text
//! pg git                          the screen (in a terminal), else the status
//! pg git connect                  sign in, find or choose the repository
//! pg git connect --new            a new private repository: paragma-memory
//! pg git connect --new acme-org/paragma-memory   a new one in an organization
//! pg git connect --repo me/app    a Paragma/ folder in a repository (--no-branch: on its main branch)
//! pg git status | sync | disconnect
//! pg git env on|off               whether the .env files go too
//! pg git move --repo me/other     the memory moves (--remove-old: out of the old one too)
//! ```

use super::{Context, USAGE, fail};
use crate::sync::github::{Account, Repo};
use crate::sync::{self, BRANCH, Config, FOLDER, NEW_REPO, Status};
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    what: Option<What>,
}

#[derive(clap::Subcommand)]
enum What {
    /// Connect the memory to a GitHub repository (found again, new, or one you choose)
    Connect(Target),
    /// Where the memory goes and how the last sync went
    Status,
    /// Sync now: send what changed here, bring in what other computers sent
    Sync {
        /// Started by pg itself after a change: waits for a quiet moment
        #[arg(long, hide = true)]
        background: bool,
    },
    /// Whether the .env files (API keys) go too: on or off
    Env { setting: String },
    /// Move the memory to another repository, or a new private one
    Move {
        #[command(flatten)]
        target: Target,
        /// Take the memory out of the old repository (it stays in its history)
        #[arg(long)]
        remove_old: bool,
    },
    /// Stop syncing on this computer (the projects and the repository stay)
    Disconnect,
}

#[derive(clap::Args, Clone, Default)]
pub struct Target {
    /// A repository you can push to: owner/name
    #[arg(long, value_name = "OWNER/NAME", conflicts_with = "new")]
    repo: Option<String>,
    /// A new private repository (default name: paragma-memory); OWNER/NAME makes it in an organization
    #[arg(long, value_name = "[OWNER/]NAME", num_args = 0..=1, default_missing_value = NEW_REPO)]
    new: Option<String>,
    /// In a repository that holds other things: the memory on its own branch (paragma); default
    #[arg(long, conflicts_with = "no_branch")]
    branch: bool,
    /// ... or on the repository's main branch, in a Paragma/ folder
    #[arg(long)]
    no_branch: bool,
    /// Send the .env files too (never to a public repository)
    #[arg(long)]
    env: bool,
    /// Yes to the questions: a public repository, joining a memory found there
    #[arg(long)]
    yes: bool,
    /// A plain git remote instead of GitHub (a path or URL)
    #[arg(long, hide = true)]
    remote: Option<String>,
    /// With --remote: a repository that holds other things
    #[arg(long, hide = true)]
    shared: bool,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    match a.what {
        None => {
            use std::io::IsTerminal;
            if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
                super::git_screen::run(cx)
            } else {
                status(cx)
            }
        }
        Some(What::Status) => status(cx),
        Some(What::Connect(t)) => connect(cx, &t),
        Some(What::Sync { background: true }) => sync::background(&cx.home),
        Some(What::Sync { background: false }) => {
            need(cx)?;
            let s = sync::sync_now(&cx.home)?;
            println!("ok synced: {}", s.did);
            conflicts(&s);
            Ok(())
        }
        Some(What::Env { setting }) => env(cx, &setting),
        Some(What::Move { target, remove_old }) => move_to(cx, &target, remove_old),
        Some(What::Disconnect) => {
            let c = need(cx)?;
            sync::disconnect(&cx.home)?;
            println!("ok disconnected from {}: the projects stay here, the memory stays there", c.place());
            Ok(())
        }
    }
}

fn need(cx: &Context) -> Result<Config> {
    Config::load(&cx.home).ok_or_else(|| fail(USAGE, "not connected: pg git connect"))
}

pub fn status(cx: &Context) -> Result<()> {
    let Some(c) = Config::load(&cx.home) else {
        println!("not connected: the memory is only on this computer. Back it up to GitHub: pg git connect");
        return Ok(());
    };
    let s = Status::load(&cx.home);
    let vis = if c.public { "public" } else { "private" };
    let who = if c.account.is_empty() { String::new() } else { format!(", account {}", c.account) };
    println!("ok git: {} ({vis}){who}", c.place());
    println!(".env files: {}", if c.env { "sent (pg git env off)" } else { "not sent (pg git env on)" });
    for line in status_lines(&s) {
        println!("{line}");
    }
    Ok(())
}

/// The last sync, for people: when and what, the error if any.
pub fn status_lines(s: &Status) -> Vec<String> {
    let now = crate::output::now();
    let mut out = Vec::new();
    if s.synced > 0 {
        out.push(format!("last sync: {}: {}", crate::output::ago(s.synced, now), s.did));
    } else {
        out.push("last sync: not yet".into());
    }
    if !s.error.is_empty() {
        out.push(format!("last try failed ({}): {}", crate::output::ago(s.tried, now), s.error));
    }
    if !s.conflicts.is_empty() {
        out.push(format!("changed on two computers, the other version kept beside: {}", s.conflicts.join(", ")));
    }
    out
}

fn conflicts(s: &Status) {
    if !s.conflicts.is_empty() {
        println!("changed on two computers, the other version kept beside: {}", s.conflicts.join(", "));
    }
}

fn env(cx: &Context, setting: &str) -> Result<()> {
    let mut c = need(cx)?;
    let on = match setting {
        "on" | "yes" | "true" => true,
        "off" | "no" | "false" => false,
        _ => return Err(fail(USAGE, "pg git env on|off")),
    };
    if on && c.public {
        return Err(fail(USAGE, format!("{} is public: .env files never go there", c.place())));
    }
    c.env = on;
    c.env_at = crate::output::now();
    c.save(&cx.home)?;
    println!("ok .env files: {}", if on { "sent from the next sync" } else { "not sent from the next sync" });
    if !on {
        println!("those already sent stay in the repository's history");
    }
    sync::trigger(&cx.home);
    Ok(())
}

/// The config for a target given on the command line, the account it
/// needs (None for a plain remote), and the repository's details.
fn config_for(t: &Target) -> Result<(Config, Option<Account>)> {
    if let Some(remote) = &t.remote {
        let (prefix, branch) = if t.shared {
            (FOLDER.to_string(), if t.no_branch { "main".to_string() } else { BRANCH.to_string() })
        } else {
            (String::new(), "main".to_string())
        };
        let c = Config {
            repo: String::new(),
            remote: remote.clone(),
            branch,
            prefix,
            env: t.env,
            env_at: if t.env { crate::output::now() } else { 0 },
            public: false,
            account: String::new(),
            gh: String::new(),
            connected: crate::output::now(),
        };
        return Ok((c, None));
    }
    let account = match Account::saved()? {
        Some(a) => a,
        None => {
            return Err(fail(
                USAGE,
                "not signed in to GitHub: run pg git in a terminal to sign in, or sign in with the GitHub CLI (gh auth login)",
            ));
        }
    };
    let repo = match (&t.new, &t.repo) {
        (Some(name), _) => account.create(name)?,
        (None, Some(name)) => {
            let repos = account.repos()?;
            match repos.into_iter().find(|r| r.full_name.eq_ignore_ascii_case(name)) {
                Some(r) => r,
                None => {
                    return Err(fail(
                        USAGE,
                        format!("no repository {name} you can push to (pg git connect lists them in a terminal)"),
                    ));
                }
            }
        }
        (None, None) => {
            // the memory found again, else a new repository
            let repos = account.repos()?;
            match account.find_memory(&repos)? {
                Some((r, prefix, branch)) => {
                    let c = config_of(&account, &r, prefix, branch, t.env.then_some(true));
                    return Ok((c, Some(account)));
                }
                None => {
                    return Err(fail(
                        USAGE,
                        "no Paragma memory found in your repositories: pg git connect --new (a private paragma-memory) or --repo owner/name",
                    ));
                }
            }
        }
    };
    // a memory's own repository (made by pg git, or holding one at its
    // root): the projects at its root, not in a Paragma/ folder
    let own = t.new.is_some() || repo.full_name.rsplit('/').next() == Some(NEW_REPO) || account.is_memory(&repo);
    let (prefix, branch) = if own {
        (String::new(), repo.default_branch.clone())
    } else if t.no_branch {
        (FOLDER.to_string(), repo.default_branch.clone())
    } else {
        (FOLDER.to_string(), BRANCH.to_string())
    };
    let c = config_of(&account, &repo, prefix, branch, t.env.then_some(true));
    Ok((c, Some(account)))
}

/// `env`: what the user chose now; None takes the memory's setting (off
/// for a new one).
pub fn config_of(account: &Account, r: &Repo, prefix: String, branch: String, env: Option<bool>) -> Config {
    let (env, env_at) = match env {
        Some(on) => (on, crate::output::now()),
        None => (false, 0),
    };
    Config {
        repo: r.full_name.clone(),
        remote: r.clone_url.clone(),
        branch,
        prefix,
        env: env && r.private,
        env_at,
        public: !r.private,
        account: account.login.clone(),
        gh: account.gh.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        connected: crate::output::now(),
    }
}

fn connect(cx: &Context, t: &Target) -> Result<()> {
    let (c, _account) = config_for(t)?;
    if c.public && !t.yes {
        return Err(fail(
            USAGE,
            format!(
                "{} is public: anyone could read the memory. We do not recommend it; to go on anyway add --yes",
                c.repo
            ),
        ));
    }
    let lines = start(cx, &c, t.yes)?;
    for l in lines {
        println!("{l}");
    }
    Ok(())
}

/// Connects to `c`: a memory already there is joined with this one (when
/// `join`), else this one is sent. What happened, for people.
pub fn start(cx: &Context, c: &Config, join: bool) -> Result<Vec<String>> {
    let _lock = sync::SyncLock::take(&cx.home, std::time::Duration::from_secs(120))?;
    let had = Config::load(&cx.home);
    c.save(&cx.home)?;
    let result = (|| -> Result<Vec<String>> {
        let m = sync::repo::Mirror::new(&cx.home, c)?;
        if m.memory_at_root() {
            anyhow::bail!("{} holds a Paragma memory at its root: connect to it as it is", c.repo);
        }
        let vis = if c.public { "public" } else { "private" };
        let mut out = vec![format!("ok connected: {} ({vis})", c.place())];
        let outcome = if m.has_memory() {
            if !join && !cx.home.projects().is_empty() {
                anyhow::bail!("{} already holds a Paragma memory: to join it with this one add --yes", c.place());
            }
            let o = m.restore()?;
            out.push(format!("a memory was there: joined with this one ({} from there, {} from here)", o.got, o.sent));
            o
        } else {
            let o = m.sync()?;
            out.push(format!("sent {}", if o.sent == 1 { "1 file".to_string() } else { format!("{} files", o.sent) }));
            o
        };
        let mut s = Status::load(&cx.home);
        s.synced = crate::output::now();
        s.tried = s.synced;
        s.error.clear();
        s.did = outcome.said();
        s.conflicts = outcome.conflicts;
        let _ = s.save(&cx.home);
        out.push(format!(".env files: {}", if c.env { "sent" } else { "not sent (pg git env on)" }));
        out.push("from now on every change here goes there by itself, and what other computers send comes here".into());
        Ok(out)
    })();
    if result.is_err() {
        // not connected after all: as it was
        match had {
            Some(h) => h.save(&cx.home)?,
            None => {
                let _ = std::fs::remove_file(sync::dir(&cx.home).join("config.json"));
            }
        }
    }
    result
}

fn move_to(cx: &Context, t: &Target, remove_old: bool) -> Result<()> {
    let old = need(cx)?;
    let (new, _account) = config_for(t)?;
    if new.remote == old.remote && new.prefix == old.prefix && new.branch == old.branch {
        return Err(fail(USAGE, format!("the memory is already in {}", old.place())));
    }
    if new.public && !t.yes {
        return Err(fail(
            USAGE,
            format!("{} is public: anyone could read the memory; to go on anyway add --yes", new.repo),
        ));
    }
    for l in move_memory(cx, &old, &new, remove_old)? {
        println!("{l}");
    }
    Ok(())
}

/// Moves the memory from `old` to `new`; what happened, for people.
pub fn move_memory(cx: &Context, old: &Config, new: &Config, remove_old: bool) -> Result<Vec<String>> {
    let mut out = Vec::new();
    {
        let _lock = sync::SyncLock::take(&cx.home, std::time::Duration::from_secs(120))?;
        let m = sync::repo::Mirror::new(&cx.home, old)?;
        // what is new here goes to the old place first, then the note
        let _ = m.sync();
        m.moved(new, remove_old)?;
        sync::remove_clone(&sync::checkout(&cx.home))?;
    }
    out.extend(start(cx, new, true)?);
    out.push(format!(
        "{} says where the memory went: other computers follow by themselves{}",
        old.place(),
        if remove_old { "; the memory's files left it (they stay in its history)" } else { "" }
    ));
    if remove_old && old.env && old.public {
        out.push("it was public: change the keys that were in its .env files".into());
    }
    Ok(out)
}
