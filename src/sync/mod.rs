//! `pg git`: the memory backed up to a git repository (on GitHub) and kept
//! the same on every computer connected to it.
//!
//! What goes: every project folder under `<home>/projects/` with its notes,
//! `.archive/` included; the `.env` files only when the user turned that on.
//! What never goes: the indexes and `state.db` in `.pg/` (rebuilt, or
//! short-lived) and the folder links (`.links`, paths of this computer).
//!
//! How: pg keeps its own clone in `<home>/git/checkout/` and never touches
//! the user's code folders. A sync copies the projects into the clone,
//! commits, fetches and merges what other computers sent, copies their
//! changes back into the projects, and pushes. A command that may change
//! memory starts a sync in the background (`trigger`); it waits a moment
//! so a burst of writes becomes one commit, and never makes the agent wait.

pub mod github;
pub mod repo;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A run of writes ends when nothing changed for this long; then it syncs.
const QUIET: std::time::Duration = std::time::Duration::from_secs(3);
/// A sync in the background waits at most this long for a quiet moment.
const MOST_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// The name of a new repository for the memory.
pub const NEW_REPO: &str = "paragma-memory";
/// The folder of the memory in a repository that holds other things.
pub const FOLDER: &str = "Paragma";
/// The branch of the memory, when it is kept apart in such a repository.
pub const BRANCH: &str = "paragma";
/// The GitHub topic that finds the memory's repository again.
pub const TOPIC: &str = "paragma-memory";
/// The file that marks where the memory is in a repository.
pub const MARKER: &str = ".paragma-memory";

/// Where the memory goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// owner/name on GitHub; empty for a plain git remote (tests).
    pub repo: String,
    /// What git pushes to: a clone URL, or a path.
    pub remote: String,
    pub branch: String,
    /// "" when the repository is the memory's own, else `FOLDER`.
    pub prefix: String,
    /// The `.env` files go too.
    pub env: bool,
    /// When that was set: the setting changed last, on any computer, counts.
    pub env_at: i64,
    pub public: bool,
    pub account: String,
    /// git signs in with the GitHub CLI (its path), not its own helper.
    pub gh: String,
    pub connected: i64,
}

/// How the last syncs went.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// When a sync last ended well (seconds).
    pub synced: i64,
    /// When the last sync tried, and what went wrong; empty when nothing.
    pub tried: i64,
    pub error: String,
    /// What the last sync did: "sent 3 changes", "nothing new".
    pub did: String,
    /// Files both sides changed, kept as they are here (theirs next to them).
    pub conflicts: Vec<String>,
}

pub fn dir(home: &crate::project::Home) -> PathBuf {
    home.root.join("git")
}

pub fn checkout(home: &crate::project::Home) -> PathBuf {
    dir(home).join("checkout")
}

impl Config {
    pub fn load(home: &crate::project::Home) -> Option<Config> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(dir(home).join("config.json")).ok()?).ok()?;
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let remote = s("remote");
        if remote.is_empty() {
            return None;
        }
        Some(Config {
            repo: s("repo"),
            remote,
            branch: s("branch"),
            prefix: s("prefix"),
            env: v["env"].as_bool().unwrap_or(false),
            env_at: v["env_at"].as_i64().unwrap_or(0),
            public: v["public"].as_bool().unwrap_or(false),
            account: s("account"),
            gh: s("gh"),
            connected: v["connected"].as_i64().unwrap_or(0),
        })
    }

    pub fn save(&self, home: &crate::project::Home) -> Result<()> {
        let v = json!({
            "repo": self.repo, "remote": self.remote, "branch": self.branch, "prefix": self.prefix,
            "env": self.env, "env_at": self.env_at, "public": self.public, "account": self.account, "gh": self.gh,
            "connected": self.connected,
        });
        crate::project::files::write_atomic(
            &dir(home).join("config.json"),
            serde_json::to_string_pretty(&v)?.as_bytes(),
        )
    }

    /// Where, for people: "me/paragma-memory", "me/app (Paragma/, branch paragma)".
    pub fn place(&self) -> String {
        let name = if self.repo.is_empty() { self.remote.clone() } else { self.repo.clone() };
        match (self.prefix.is_empty(), self.branch == BRANCH) {
            (true, _) => name,
            (false, true) => format!("{name} ({FOLDER}/, branch {BRANCH})"),
            (false, false) => format!("{name} ({FOLDER}/ on {})", self.branch),
        }
    }
}

impl Status {
    pub fn load(home: &crate::project::Home) -> Status {
        let Some(v) = std::fs::read_to_string(dir(home).join("status.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            return Status::default();
        };
        Status {
            synced: v["synced"].as_i64().unwrap_or(0),
            tried: v["tried"].as_i64().unwrap_or(0),
            error: v["error"].as_str().unwrap_or("").to_string(),
            did: v["did"].as_str().unwrap_or("").to_string(),
            conflicts: v["conflicts"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
        }
    }

    pub fn save(&self, home: &crate::project::Home) -> Result<()> {
        let v = json!({
            "synced": self.synced, "tried": self.tried, "error": self.error, "did": self.did,
            "conflicts": self.conflicts,
        });
        crate::project::files::write_atomic(
            &dir(home).join("status.json"),
            serde_json::to_string_pretty(&v)?.as_bytes(),
        )
    }
}

/// One sync at a time per home, across processes: held until dropped.
pub struct SyncLock {
    _file: std::fs::File,
}

impl SyncLock {
    /// None when another sync holds it.
    pub fn try_take(home: &crate::project::Home) -> Result<Option<SyncLock>> {
        std::fs::create_dir_all(dir(home))?;
        let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir(home).join("lock"))?;
        match f.try_lock() {
            Ok(()) => Ok(Some(SyncLock { _file: f })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => Err(e).context("lock the sync"),
        }
    }

    /// Waits for the sync running now to end (at most `wait`).
    pub fn take(home: &crate::project::Home, wait: std::time::Duration) -> Result<SyncLock> {
        let start = std::time::Instant::now();
        loop {
            if let Some(l) = SyncLock::try_take(home)? {
                return Ok(l);
            }
            if start.elapsed() > wait {
                anyhow::bail!("another sync kept running for {}s", wait.as_secs());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

/// When memory last changed, as told by `trigger` (milliseconds).
fn dirty_at(home: &crate::project::Home) -> u128 {
    std::fs::read_to_string(dir(home).join("dirty")).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

/// After a command that may have changed memory (or should bring in what
/// other computers sent): notes the moment, and starts a sync in the
/// background unless one is running, which then sees the new moment.
/// Quick, and never fails the command.
pub fn trigger(home: &crate::project::Home) {
    if std::env::var_os("PARAGMA_NO_SYNC").is_some() || !dir(home).join("config.json").exists() {
        return;
    }
    let _ = std::fs::write(dir(home).join("dirty"), now_ms().to_string());
    // when one is running, it sees the new moment before it ends
    if let Ok(Some(lock)) = SyncLock::try_take(home) {
        drop(lock);
        let _ = spawn_background(home);
    }
}

/// `pg git sync --background` on its own, detached: no window, no handle
/// of the agent's pipes, out of the agent's process group or job.
fn spawn_background(home: &crate::project::Home) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut c = std::process::Command::new(exe);
    c.args(["git", "sync", "--background"])
        .env("PARAGMA_HOME", &home.root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    detach(&mut c)
}

#[cfg(windows)]
fn detach(c: &mut std::process::Command) -> Result<()> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    // the agent waits for its pipes to close: the child must not hold them
    unsafe {
        for h in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            SetHandleInformation(GetStdHandle(h), HANDLE_FLAG_INHERIT, 0);
        }
    }
    // out of the agent's job when it allows that (a job may end its
    // processes when the agent's command ends), else in it
    c.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB);
    if c.spawn().is_ok() {
        return Ok(());
    }
    c.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    c.spawn()?;
    Ok(())
}

#[cfg(not(windows))]
fn detach(c: &mut std::process::Command) -> Result<()> {
    use std::os::unix::process::CommandExt;
    c.process_group(0);
    c.spawn()?;
    Ok(())
}

/// The sync started by `trigger`: waits for a quiet moment, syncs, and
/// again while memory changed meanwhile. Exits when another one runs.
pub fn background(home: &crate::project::Home) -> Result<()> {
    let Some(_lock) = SyncLock::try_take(home)? else { return Ok(()) };
    let started = std::time::Instant::now();
    loop {
        // the quiet moment
        loop {
            let since = now_ms().saturating_sub(dirty_at(home));
            if since >= QUIET.as_millis() || started.elapsed() > MOST_WAIT {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let seen = dirty_at(home);
        let _ = sync_locked(home);
        if dirty_at(home) == seen {
            return Ok(());
        }
    }
}

/// A sync now, waiting for one already running (`pg git sync`).
pub fn sync_now(home: &crate::project::Home) -> Result<Status> {
    let _lock = SyncLock::take(home, std::time::Duration::from_secs(120))?;
    sync_locked(home)
}

/// One sync, the lock held; its outcome is kept in status.json.
fn sync_locked(home: &crate::project::Home) -> Result<Status> {
    let Some(config) = Config::load(home) else { anyhow::bail!("not connected: pg git connect") };
    let mut status = Status::load(home);
    status.tried = crate::output::now();
    let result = repo::Mirror::new(home, &config).and_then(|m| m.sync());
    match &result {
        Ok(outcome) => {
            status.synced = status.tried;
            status.error.clear();
            status.did = outcome.said();
            status.conflicts = outcome.conflicts.clone();
            if let Some(to) = &outcome.moved_to {
                follow_move(home, &config, to)?;
                status.did = format!("the memory moved to {to}: following it");
            }
        }
        Err(e) => status.error = format!("{e:#}"),
    }
    status.save(home)?;
    result.map(|_| status)
}

/// Another computer moved the memory: this one goes there too.
fn follow_move(home: &crate::project::Home, old: &Config, to: &str) -> Result<()> {
    let mut new = old.clone();
    if let Some((repo, remote, prefix, branch)) = repo::parse_moved(to) {
        new.repo = repo;
        new.remote = remote;
        new.prefix = prefix;
        new.branch = branch;
        new.save(home)?;
        let _ = std::fs::remove_dir_all(checkout(home));
        repo::Mirror::new(home, &new)?.sync()?;
    }
    Ok(())
}

/// Takes the memory off git on this computer: the projects stay as they
/// are, so does the repository; only pg's own clone and settings go.
pub fn disconnect(home: &crate::project::Home) -> Result<()> {
    let _lock = SyncLock::take(home, std::time::Duration::from_secs(120))?;
    for f in ["config.json", "status.json", "dirty"] {
        let _ = std::fs::remove_file(dir(home).join(f));
    }
    remove_clone(&checkout(home))
}

/// Removes pg's clone: read-only files in .git (packs on Windows) made
/// writable first.
pub fn remove_clone(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if std::fs::remove_dir_all(path).is_ok() {
        return Ok(());
    }
    fn writable(p: &Path) {
        if let Ok(entries) = std::fs::read_dir(p) {
            for e in entries.flatten() {
                let path = e.path();
                if path.is_dir() {
                    writable(&path);
                } else if let Ok(meta) = std::fs::metadata(&path) {
                    let mut perm = meta.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    let _ = std::fs::set_permissions(&path, perm);
                }
            }
        }
    }
    writable(path);
    std::fs::remove_dir_all(path).with_context(|| format!("remove {}", path.display()))
}
