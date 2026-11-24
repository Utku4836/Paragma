//! `pg sync` (also `pg git`): the memory kept in one place and the same on
//! every computer connected to it. The place is a git repository (on
//! GitHub, or any git host by its address), a cloud folder on this computer
//! (OneDrive, Google Drive, Dropbox, iCloud: sync/cloud.rs, sync/store.rs)
//! or a WebDAV server (sync/webdav.rs). Backups of the whole memory into one
//! zip, and their restore, are in sync/backup.rs; moving the memory from one
//! place to another, checked, in sync/transfer.rs.
//!
//! Each memory has an id, kept in a marker file in every place that holds
//! it; a place it moved from keeps a note of where it went, so the other
//! computers follow by themselves.
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

pub mod backup;
pub mod browse;
pub mod cloud;
pub mod fsx;
pub mod github;
pub mod history;
pub mod preflight;
pub mod progress;
pub mod repo;
pub mod store;
pub mod transfer;
pub mod webdav;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A run of writes ends when nothing changed for this long; then it syncs.
const QUIET: std::time::Duration = std::time::Duration::from_secs(3);
/// A sync in the background waits at most this long for a quiet moment.
const MOST_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// The size limit of a file kept with a note that syncs, by default.
pub const ATTACH_MB: u64 = 20;
/// The most it can be: GitHub refuses files of 100 MB.
pub const MOST_MB: u64 = 95;

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
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Config {
    /// "github", "git" (any host by its address), "folder" (a cloud
    /// folder, or any folder), "webdav".
    pub kind: String,
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
    /// Files kept with notes go too (pg write --file), up to this size.
    pub attachments: bool,
    pub attach_mb: u64,
    pub account: String,
    /// git signs in with the GitHub CLI (its path), not its own helper.
    pub gh: String,
    pub connected: i64,
    /// A folder: its cloud ("onedrive", "gdrive", "dropbox", "icloud"; ""
    /// for a folder of no cloud) and its path below the cloud's folder (the
    /// whole path for no cloud). `remote` is where that is on this computer.
    pub cloud: String,
    pub path: String,
    /// The memory's id: the same in every place holding it.
    pub id: String,
    /// Projects that stay on this computer: neither sent nor brought in.
    pub skip: Vec<String>,
    /// Syncing paused on this computer: nothing goes or comes until resumed.
    pub paused: bool,
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
        let kind = match s("kind") {
            k if !k.is_empty() => k,
            _ if !s("repo").is_empty() => "github".to_string(),
            _ => "git".to_string(),
        };
        Some(Config {
            kind,
            cloud: s("cloud"),
            path: s("path"),
            id: s("id"),
            repo: s("repo"),
            remote,
            branch: s("branch"),
            prefix: s("prefix"),
            env: v["env"].as_bool().unwrap_or(false),
            env_at: v["env_at"].as_i64().unwrap_or(0),
            public: v["public"].as_bool().unwrap_or(false),
            attachments: v["attachments"].as_bool().unwrap_or(false),
            attach_mb: v["attach_mb"].as_u64().unwrap_or(ATTACH_MB).clamp(1, MOST_MB),
            account: s("account"),
            gh: s("gh"),
            connected: v["connected"].as_i64().unwrap_or(0),
            skip: v["skip"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            paused: v["paused"].as_bool().unwrap_or(false),
        })
    }

    /// Whether a file of the projects goes with the memory from this
    /// computer (its project is not one that stays here).
    pub fn goes(&self, rel: &str) -> bool {
        let project = rel.split('/').next().unwrap_or(rel);
        !self.skip.iter().any(|s| s == project)
    }

    /// The config of the usual home, for a choice made before (the size
    /// limit) when there is no home at hand.
    pub fn load_any() -> Option<Config> {
        Config::load(&crate::project::Home::locate())
    }

    pub fn save(&self, home: &crate::project::Home) -> Result<()> {
        let v = json!({
            "repo": self.repo, "remote": self.remote, "branch": self.branch, "prefix": self.prefix,
            "env": self.env, "env_at": self.env_at, "public": self.public, "account": self.account, "gh": self.gh,
            "connected": self.connected, "attachments": self.attachments, "attach_mb": self.attach_mb,
            "kind": self.kind, "cloud": self.cloud, "path": self.path, "id": self.id,
            "skip": self.skip, "paused": self.paused,
        });
        crate::project::files::write_atomic(
            &dir(home).join("config.json"),
            serde_json::to_string_pretty(&v)?.as_bytes(),
        )?;
        // the memory's id outlives the connection: connected again later,
        // an older copy of it is told from another memory
        if !self.id.is_empty() {
            crate::project::files::write_atomic(&dir(home).join("id"), self.id.as_bytes())?;
        }
        Ok(())
    }

    /// Where, for people: "me/paragma-memory", "me/app (Paragma/, branch
    /// paragma)", "OneDrive: Paragma", "WebDAV cloud.example.com/…".
    pub fn place(&self) -> String {
        match self.kind.as_str() {
            "folder" if self.cloud.is_empty() => return self.remote.clone(),
            "folder" => return format!("{}: {}", cloud::name(&self.cloud), self.path),
            "webdav" => {
                return format!("WebDAV {}", self.remote.split_once("://").map(|(_, r)| r).unwrap_or(&self.remote));
            }
            _ => {}
        }
        let name = if self.repo.is_empty() { self.remote.clone() } else { self.repo.clone() };
        match (self.prefix.is_empty(), self.branch == BRANCH) {
            (true, _) => name,
            (false, true) => format!("{name} ({}/, branch {BRANCH})", self.prefix),
            (false, false) => format!("{name} ({}/ on {})", self.prefix, self.branch),
        }
    }
}

impl Config {
    /// Kept in files (a folder, WebDAV), not in git.
    pub fn files(&self) -> bool {
        matches!(self.kind.as_str(), "folder" | "webdav")
    }

    /// Where the memory is, as the marker of a place it moved from says it.
    pub fn place_json(&self) -> Value {
        json!({
            "kind": self.kind, "repo": self.repo, "remote": self.remote, "prefix": self.prefix,
            "branch": self.branch, "cloud": self.cloud, "path": self.path, "account": self.account, "id": self.id,
        })
    }

    /// This config moved to the place a marker names; None when it names
    /// none.
    pub fn moved_to(&self, to: &Value) -> Option<Config> {
        let s = |k: &str| to[k].as_str().unwrap_or("").to_string();
        let mut c = self.clone();
        c.kind = match s("kind") {
            k if !k.is_empty() => k,
            _ if !s("repo").is_empty() => "github".into(),
            _ => "git".into(),
        };
        c.repo = s("repo");
        c.remote = s("remote");
        c.prefix = s("prefix");
        c.branch = s("branch");
        c.cloud = s("cloud");
        c.path = s("path");
        if c.kind == "webdav" {
            c.account = s("account");
        }
        if !s("id").is_empty() {
            c.id = s("id");
        }
        (!c.remote.is_empty() || !c.path.is_empty()).then_some(c)
    }
}

/// A new memory's id.
/// This memory's id: the connected place's, else the one it had when it
/// was last connected; empty when it never was.
pub fn memory_id(home: &crate::project::Home) -> String {
    Config::load(home)
        .map(|c| c.id)
        .filter(|i| !i.is_empty())
        .or_else(|| std::fs::read_to_string(dir(home).join("id")).ok().map(|s| s.trim().to_string()))
        .unwrap_or_default()
}

pub fn new_id() -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut h);
    std::process::id().hash(&mut h);
    repo::host().hash(&mut h);
    format!("{:016x}", h.finish())
}

/// The place a config names, as a store of files (a folder, WebDAV); the
/// folder found on this computer (its cloud's folder here).
pub fn store_of(home: &crate::project::Home, c: &Config) -> Result<Box<dyn store::Store>> {
    match c.kind.as_str() {
        "folder" => {
            let root = cloud::resolve(&c.cloud, &c.path, &c.id).ok_or_else(|| {
                if c.cloud.is_empty() {
                    anyhow::anyhow!("the folder {} is not there", c.path)
                } else {
                    let state = cloud::state(&c.cloud);
                    let hint =
                        state.hint().unwrap_or_else(|| format!("{} holds no memory of this id", cloud::name(&c.cloud)));
                    anyhow::anyhow!("{hint} (the memory is in its folder {})", c.path)
                }
            })?;
            Ok(Box::new(store::Folder::new(root)))
        }
        "webdav" => {
            let pw = webdav::password_of(home, &c.remote, &c.account).ok_or_else(|| {
                anyhow::anyhow!(
                    "the password for {} is not on this computer: pg sync connect webdav {} --user {}",
                    c.place(),
                    c.remote,
                    c.account
                )
            })?;
            Ok(Box::new(webdav::WebDav::new(&c.remote, &c.account, &pw, dir(home).join("tmp"))))
        }
        k => anyhow::bail!("{k} is not a place of files"),
    }
}

/// One sync with whatever place the config names.
fn sync_with(home: &crate::project::Home, c: &Config) -> Result<repo::Outcome> {
    if c.files() {
        let s = store_of(home, c)?;
        store::Mirror::new(home, c, s.as_ref()).sync()
    } else {
        repo::Mirror::new(home, c)?.sync()
    }
}

/// Forgets what this computer knew of the last place (its clone, the base
/// of the files): the next sync with a new place starts afresh.
pub fn forget_place(home: &crate::project::Home) -> Result<()> {
    let _ = std::fs::remove_file(dir(home).join("base.json"));
    let _ = std::fs::remove_dir_all(dir(home).join("base"));
    remove_clone(&checkout(home))
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
        crate::sync::fsx::create_dir_all(dir(home))?;
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
    if Config::load(home).is_some_and(|c| c.paused) {
        return;
    }
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
pub(crate) fn detach(c: &mut std::process::Command) -> Result<()> {
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
pub(crate) fn detach(c: &mut std::process::Command) -> Result<()> {
    use std::os::unix::process::CommandExt;
    c.process_group(0);
    c.spawn()?;
    Ok(())
}

/// The sync started by `trigger`: waits for a quiet moment, syncs, and
/// again while memory changed meanwhile. Exits when another one runs.
pub fn background(home: &crate::project::Home) -> Result<()> {
    if Config::load(home).is_some_and(|c| c.paused) {
        return Ok(());
    }
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
    let _done = progress::Done(home);
    let Some(config) = Config::load(home) else { anyhow::bail!("not connected: pg git connect") };
    let mut status = Status::load(home);
    status.tried = crate::output::now();
    let result = sync_with(home, &config);
    match &result {
        Ok(outcome) => {
            status.synced = status.tried;
            status.error.clear();
            status.did = outcome.said();
            status.conflicts = outcome.conflicts.clone();
            if let Some(to) = &outcome.moved_to {
                let place = serde_json::from_str::<Value>(to)
                    .ok()
                    .and_then(|v| config.moved_to(&v))
                    .map(|c| c.place())
                    .unwrap_or_default();
                match follow_move(home, &config, to) {
                    Ok(()) => status.did = format!("the memory moved to {place}: followed it here"),
                    Err(e) => {
                        status.error = format!(
                            "the memory moved to {place}; this computer could not follow: {}",
                            crate::output::error_text(&e)
                        );
                        status.save(home)?;
                        return Err(e);
                    }
                }
            }
        }
        Err(e) => status.error = crate::output::error_text(&e),
    }
    status.save(home)?;
    if !status.error.is_empty() {
        history::add(home, &format!("sync failed: {}", status.error), false);
    } else if status.did != repo::Outcome::default().said() {
        history::add(home, &status.did, true);
    }
    result.map(|_| status)
}

/// Another computer moved the memory: this one goes there too, when it
/// can reach the new place (its cloud's folder is here, the password is
/// here); else it keeps working here and says what it needs.
fn follow_move(home: &crate::project::Home, old: &Config, to: &str) -> Result<()> {
    let Ok(v) = serde_json::from_str::<Value>(to) else { return Ok(()) };
    let Some(mut new) = old.moved_to(&v) else { return Ok(()) };
    if new.files() {
        // where it is on this computer
        let s = store_of(home, &new).with_context(|| format!("the memory moved to {}", new.place()))?;
        if new.kind == "folder" {
            new.remote = s.place();
        }
    }
    new.save(home)?;
    forget_place(home)?;
    if new.files() {
        let s = store_of(home, &new)?;
        store::Mirror::new(home, &new, s.as_ref()).restore()?;
    } else {
        repo::Mirror::new(home, &new)?.sync()?;
    }
    Ok(())
}

/// Takes the memory off git on this computer: the projects stay as they
/// are, so does the repository; only pg's own clone and settings go.
pub fn disconnect(home: &crate::project::Home) -> Result<()> {
    let _lock = SyncLock::take(home, std::time::Duration::from_secs(120))?;
    if let Some(c) = Config::load(home)
        && c.kind == "webdav"
    {
        webdav::forget_password(home, &c.remote, &c.account);
    }
    if let Some(c) = Config::load(home) {
        history::add(home, &format!("disconnected from {}", c.place()), true);
    }
    for f in ["config.json", "status.json", "dirty"] {
        let _ = std::fs::remove_file(dir(home).join(f));
    }
    forget_place(home)
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
