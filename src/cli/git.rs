//! pg sync (also pg git): the memory kept in one place and the same on
//! every computer (see sync/): a GitHub repository, any git host by its
//! address, a cloud folder on this computer, a WebDAV server; backups of
//! the whole memory, and their restore.
//!
//! ```text
//! pg sync                          where the memory goes and how the last sync went
//! pg git                           the GitHub screen (in a terminal)
//! pg sync connect                  GitHub: sign in, the memory found again
//! pg sync connect --new            a new private repository on GitHub: paragma-memory
//! pg sync connect --repo me/app    a Paragma/ folder in a repository (--no-branch: on its main branch)
//! pg sync connect git https://gitlab.com/me/notes.git   any git host
//! pg sync connect onedrive         a cloud folder (gdrive, dropbox, icloud; --folder NAME, default Paragma)
//! pg sync connect folder D:\Sync\Paragma                any folder
//! pg sync connect webdav https://cloud.example.com/remote.php/dav/files/me/Paragma --user me
//! pg sync move <where>             the memory moves, checked (--remove-old: out of the old place too)
//!   --there join|update|replace|take   a memory found there: joined, made this one, or this one made it
//! pg sync remove-there <where>     a memory in a place taken out (zipped first), nothing connected
//! pg sync now | disconnect
//! pg sync env on|off               whether the .env files go too
//! pg sync files on|off --limit MB  whether the files kept with notes go too
//! pg sync backup [<folder>]        the whole memory in one zip (default: Downloads)
//! pg sync backup --daily on --to <folder>   one a day, the last 7 kept
//! pg sync backups                  the backups on this computer
//! pg sync restore <zip> [--replace]         a backup back (joined with the memory, or in its place)
//! ```

use super::{Context, USAGE, fail};
use crate::sync::github::{Account, Repo};
use crate::sync::transfer::There;
use crate::sync::{self, BRANCH, Config, FOLDER, NEW_REPO, Status, backup, cloud};
use anyhow::{Context as _, Result};
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    what: Option<What>,
}

#[derive(clap::Subcommand)]
enum What {
    /// Connect the memory to a place: GitHub (default), git <address>, a cloud folder (onedrive, gdrive, dropbox,
    /// icloud), folder <path>, webdav <address>
    Connect(Target),
    /// Where the memory goes and how the last sync went
    Status,
    /// Sync now: send what changed here, bring in what other computers sent
    #[command(alias = "now")]
    Sync {
        /// Started by pg itself after a change: waits for a quiet moment
        #[arg(long, hide = true)]
        background: bool,
    },
    /// Whether the .env files (API keys) go too: on or off
    Env { setting: String },
    /// Whether the files kept with notes (pg write --file) go too: on or off, and up to what size
    Files {
        /// on or off (alone: how it is now)
        setting: Option<String>,
        /// The size limit of one file, in MB (1 to 95; GitHub refuses 100 MB)
        #[arg(long, value_name = "MB")]
        limit: Option<u64>,
    },
    /// Move the memory to another place, of any kind; checked before the old place is left
    Move {
        #[command(flatten)]
        target: Target,
        /// Take the memory out of the old place (a git repository keeps it in its history)
        #[arg(long)]
        remove_old: bool,
    },
    /// Take a memory out of a place (an older copy left there): zipped into Downloads first; nothing is connected
    RemoveThere(Target),
    /// Stop syncing on this computer (the projects and the place stay)
    Disconnect,
    /// The whole memory in one zip: in a folder (default: Downloads), or one a day (--daily on --to <folder>)
    Backup {
        /// Where the zip goes
        folder: Option<PathBuf>,
        /// The .env files (keys) go into it too
        #[arg(long)]
        env: bool,
        /// A backup every day, by pg's first command of the day: on or off
        #[arg(long, value_name = "on|off")]
        daily: Option<String>,
        /// With --daily on: the folder of the daily backups
        #[arg(long, value_name = "FOLDER")]
        to: Option<PathBuf>,
        /// With --daily: how many are kept (default 7)
        #[arg(long)]
        keep: Option<usize>,
        #[arg(long, hide = true)]
        daily_now: bool,
    },
    /// The Paragma backups on this computer (Downloads, Desktop, Documents, cloud folders, the daily ones)
    Backups,
    /// A backup back: joined with the memory (a note that differs kept beside it), or --replace
    Restore {
        /// The backup's zip (the newest one found when left out)
        zip: Option<PathBuf>,
        /// The memory becomes the backup (what it does not have goes)
        #[arg(long)]
        replace: bool,
        /// Yes to replacing
        #[arg(long)]
        yes: bool,
    },
}

#[derive(clap::Args, Clone, Default)]
pub struct Target {
    /// Where: github (default), git <address>, onedrive, gdrive, dropbox, icloud, folder <path>, webdav <address>
    #[arg(value_name = "WHERE")]
    place: Vec<String>,
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
    /// In a cloud or a git repository: the memory's folder in it (default: the memory found there, else Paragma)
    #[arg(long, value_name = "NAME")]
    folder: Option<String>,
    /// WebDAV: the user name
    #[arg(long)]
    user: Option<String>,
    /// WebDAV: the password (an app password) read from stdin; else asked in a terminal
    #[arg(long)]
    password_stdin: bool,
    /// Send the .env files too (never to a public repository)
    #[arg(long)]
    env: bool,
    /// Yes to the questions: a public repository, joining a memory found there
    #[arg(long)]
    yes: bool,
    /// A memory found there: join (both kept), update or replace (it becomes this one), take (this computer's
    /// becomes it); what changes is zipped into Downloads first
    #[arg(long, value_name = "join|update|replace|take")]
    there: Option<String>,
    /// A plain git remote instead of GitHub (a path or URL)
    #[arg(long, hide = true)]
    remote: Option<String>,
    /// With --remote: a repository that holds other things
    #[arg(long, hide = true)]
    shared: bool,
}

/// `screen`: pg git alone opens the GitHub screen in a terminal.
pub fn run(cx: &Context, a: Args, screen: bool) -> Result<()> {
    match a.what {
        None => {
            use std::io::IsTerminal;
            if screen && std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
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
        Some(What::Files { setting, limit }) => files(cx, setting.as_deref(), limit),
        Some(What::Move { target, remove_old }) => move_to(cx, &target, remove_old),
        Some(What::RemoveThere(t)) => {
            let c = config_for(cx, &t)?;
            for l in sync::transfer::remove_there(&cx.home, &c)? {
                println!("{l}");
            }
            Ok(())
        }
        Some(What::Disconnect) => {
            let c = need(cx)?;
            sync::disconnect(&cx.home)?;
            println!("ok disconnected from {}: the projects stay here, the memory stays there", c.place());
            Ok(())
        }
        Some(What::Backup { folder, env, daily, to, keep, daily_now }) => {
            backup_cmd(cx, folder, env, daily.as_deref(), to, keep, daily_now)
        }
        Some(What::Backups) => backups(cx),
        Some(What::Restore { zip, replace, yes }) => restore(cx, zip, replace, yes),
    }
}

/// pg sync files on|off [--limit MB]: whether the files kept with notes go
/// too, and up to what size.
fn files(cx: &Context, setting: Option<&str>, limit: Option<u64>) -> Result<()> {
    let mut c = need(cx)?;
    if let Some(s) = setting {
        let on = match s.to_ascii_lowercase().as_str() {
            "on" | "yes" | "true" => true,
            "off" | "no" | "false" => false,
            _ => return Err(fail(USAGE, "pg sync files on, or off")),
        };
        if on && c.public {
            return Err(fail(USAGE, "the repository is public: the files kept with notes never go there"));
        }
        c.attachments = on;
    }
    if let Some(mb) = limit {
        if !(1..=crate::sync::MOST_MB).contains(&mb) {
            return Err(fail(
                USAGE,
                format!("give the limit in MB, 1 to {} (GitHub refuses 100 MB)", crate::sync::MOST_MB),
            ));
        }
        c.attach_mb = mb;
    }
    if setting.is_some() || limit.is_some() {
        c.save(&cx.home)?;
        if c.attachments {
            sync::trigger(&cx.home);
        }
    }
    let on = if !c.attachments {
        "not sent: they stay on this computer"
    } else if c.files() {
        "sent with the memory"
    } else {
        "sent with the memory (a file sent to git stays in its history, also after it is taken out)"
    };
    println!("ok files kept with notes: {on}; up to {} MB a file", c.attach_mb);
    Ok(())
}

fn need(cx: &Context) -> Result<Config> {
    Config::load(&cx.home).ok_or_else(|| fail(USAGE, "not connected: pg sync connect"))
}

pub fn status(cx: &Context) -> Result<()> {
    let daily = backup::Daily::load(&cx.home);
    let Some(c) = Config::load(&cx.home) else {
        println!(
            "not connected: the memory is only on this computer. Keep it on GitHub, another git host, a cloud folder or WebDAV: pg sync connect --help"
        );
        daily_line(&daily);
        for st in cloud::states().iter().filter(|st| st.installed) {
            println!("{}: {}", st.name, st.said());
        }
        return Ok(());
    };
    let s = Status::load(&cx.home);
    let who = if c.account.is_empty() { String::new() } else { format!(", account {}", c.account) };
    let vis = match c.kind.as_str() {
        "github" if c.public => " (public)",
        "github" => " (private)",
        _ => "",
    };
    println!("ok {}: {}{vis}{who}", kind_name(&c.kind), c.place());
    if !c.cloud.is_empty() {
        let st = cloud::state(&c.cloud);
        println!("{}: {}", st.name, st.said());
        if let Some((line, _)) = cloud::upload_line(&c.cloud, &c.path, &c.id) {
            println!("{line}");
        }
    }
    println!(".env files: {}", if c.env { "sent (pg sync env off)" } else { "not sent (pg sync env on)" });
    for line in status_lines(&s) {
        println!("{line}");
    }
    daily_line(&daily);
    Ok(())
}

fn daily_line(d: &backup::Daily) {
    if d.on {
        let last = if d.last > 0 { crate::output::ago(d.last, crate::output::now()) } else { "none yet".into() };
        println!("daily backup: on, into {} (the last {} kept; last: {last})", d.folder, d.keep);
    }
}

pub fn kind_name(kind: &str) -> &'static str {
    match kind {
        "github" => "GitHub",
        "git" => "git",
        "folder" => "folder",
        "webdav" => "WebDAV",
        _ => "place",
    }
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
        _ => return Err(fail(USAGE, "pg sync env on|off")),
    };
    if on && c.public {
        return Err(fail(USAGE, format!("{} is public: .env files never go there", c.place())));
    }
    c.env = on;
    c.env_at = crate::output::now();
    c.save(&cx.home)?;
    println!("ok .env files: {}", if on { "sent from the next sync" } else { "not sent from the next sync" });
    if !on && !c.files() {
        println!("those already sent stay in the repository's history");
    }
    sync::trigger(&cx.home);
    Ok(())
}

/// The settings a target given on the command line names.
fn config_for(cx: &Context, t: &Target) -> Result<Config> {
    let fresh = |kind: &str| Config {
        kind: kind.into(),
        env: t.env,
        env_at: if t.env { crate::output::now() } else { 0 },
        attachments: Config::load(&cx.home).map(|c| c.attachments).unwrap_or(false),
        attach_mb: Config::load(&cx.home).map(|c| c.attach_mb).unwrap_or(crate::sync::ATTACH_MB),
        connected: crate::output::now(),
        ..Config::default()
    };
    if let Some(remote) = &t.remote {
        let (prefix, branch) = if t.shared {
            (FOLDER.to_string(), if t.no_branch { "main".to_string() } else { BRANCH.to_string() })
        } else {
            (String::new(), "main".to_string())
        };
        return Ok(Config { remote: remote.clone(), prefix, branch, ..fresh("git") });
    }
    let kind = t.place.first().map(|s| s.to_ascii_lowercase());
    let addr = t.place.get(1).cloned();
    let need_addr =
        |what: &str| addr.clone().ok_or_else(|| fail(USAGE, format!("say where: pg sync connect {what} <address>")));
    match kind.as_deref() {
        None | Some("github") => github_config(t),
        Some("git") => {
            let url = need_addr("git")?;
            let (prefix, branch) = git_layout(&url, t)?;
            Ok(Config { remote: url, prefix: in_folder(t, prefix), branch, ..fresh("git") })
        }
        Some("folder") => {
            let p = std::path::absolute(PathBuf::from(need_addr("folder")?))?;
            Ok(folder_config(&p, fresh("folder")))
        }
        Some(id @ ("onedrive" | "gdrive" | "google-drive" | "googledrive" | "dropbox" | "icloud")) => {
            let id = if id.contains("google") { "gdrive" } else { id };
            let state = cloud::state(id);
            if let Some(hint) = state.hint() {
                return Err(fail(USAGE, hint));
            }
            let root = state.root.context("the cloud's folder")?;
            // the memory already there, else a Paragma folder
            let p = match &t.folder {
                Some(f) => root.join(f),
                None => cloud::search(&root, "").unwrap_or_else(|| root.join(FOLDER)),
            };
            Ok(folder_config(&p, fresh("folder")))
        }
        Some("webdav") => {
            let url = need_addr("webdav")?.trim_end_matches('/').to_string();
            if !url.starts_with("https://") && !url.starts_with("http://") {
                return Err(fail(USAGE, "a WebDAV address starts with https://"));
            }
            let user = t.user.clone().ok_or_else(|| fail(USAGE, "the user name: --user <name>"))?;
            let password = password(t)?;
            let dav = sync::webdav::WebDav::new(&url, &user, &password, sync::dir(&cx.home).join("tmp"));
            dav.check()?;
            sync::webdav::save_password(&cx.home, &url, &user, &password)?;
            Ok(Config { remote: url, account: user, ..fresh("webdav") })
        }
        Some(other) => {
            let p = std::path::absolute(PathBuf::from(other))?;
            if p.is_dir() {
                return Ok(folder_config(&p, fresh("folder")));
            }
            Err(fail(
                USAGE,
                format!(
                    "{other:?} is not a place pg knows: github, git <address>, onedrive, gdrive, dropbox, icloud, folder <path>, webdav <address>"
                ),
            ))
        }
    }
}

/// A folder: its cloud and path in it, the id of a memory already there.
fn folder_config(p: &std::path::Path, base: Config) -> Config {
    let (cloud, path) = cloud::portable(p);
    Config {
        remote: p.to_string_lossy().into_owned(),
        cloud,
        path,
        id: cloud::marker_id(p).unwrap_or_default(),
        public: false,
        ..base
    }
}

/// A git repository's layout for the memory: an empty one, or one that is
/// a memory's own, holds it at its root; one that holds other things, in a
/// Paragma/ folder on its own branch (or on its main branch).
fn git_layout(url: &str, t: &Target) -> Result<(String, String)> {
    // the user may be asked to sign in here: this is their command
    let out = std::process::Command::new("git")
        .args(["ls-remote", "--symref", url])
        .output()
        .context("run git (is it installed?)")?;
    if !out.status.success() {
        return Err(fail(USAGE, format!("git could not reach {url}: {}", String::from_utf8_lossy(&out.stderr).trim())));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let default = text
        .lines()
        .find_map(|l| l.strip_prefix("ref: refs/heads/").and_then(|r| r.split_whitespace().next()))
        .unwrap_or("main")
        .to_string();
    if text.trim().is_empty() {
        return Ok((String::new(), default));
    }
    if text.contains(&format!("refs/heads/{BRANCH}")) {
        return Ok((FOLDER.to_string(), BRANCH.to_string()));
    }
    // a memory's own repository: the marker at its root
    let tmp = std::env::temp_dir().join(format!("paragma-look-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let git = |args: &[&str]| std::process::Command::new("git").args(args).current_dir(&tmp).output();
    let own = git(&["init", "-q"]).is_ok()
        && git(&["fetch", "-q", "--depth", "1", url, &default]).is_ok_and(|o| o.status.success())
        && git(&["cat-file", "-e", &format!("FETCH_HEAD:{}", sync::MARKER)]).is_ok_and(|o| o.status.success());
    let _ = sync::remove_clone(&tmp);
    Ok(if own {
        (String::new(), default)
    } else if t.no_branch {
        (FOLDER.to_string(), default)
    } else {
        (FOLDER.to_string(), BRANCH.to_string())
    })
}

/// A WebDAV password: from stdin, or asked in a terminal without showing it.
fn password(t: &Target) -> Result<String> {
    use std::io::{BufRead, IsTerminal};
    if t.password_stdin || !std::io::stdin().is_terminal() {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        let p = line.trim_end_matches(['\r', '\n']).to_string();
        if p.is_empty() {
            return Err(fail(USAGE, "no password on stdin"));
        }
        return Ok(p);
    }
    eprint!("password (an app password, not shown): ");
    let p = crate::terminal::read_hidden()?;
    eprintln!();
    Ok(p)
}

fn github_config(t: &Target) -> Result<Config> {
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
                        format!("no repository {name} you can push to (pg git lists them in a terminal)"),
                    ));
                }
            }
        }
        (None, None) => {
            // the memory found again, else a new repository
            let repos = account.repos()?;
            match account.find_memory(&repos)? {
                Some((r, prefix, branch)) => {
                    return Ok(config_of(&account, &r, prefix, branch, t.env.then_some(true), None));
                }
                None => {
                    return Err(fail(
                        USAGE,
                        "no Paragma memory found in your repositories: pg sync connect --new (a private paragma-memory) or --repo owner/name",
                    ));
                }
            }
        }
    };
    // a memory's own repository (made by pg, or holding one at its root):
    // the projects at its root, not in a Paragma/ folder
    let own = t.new.is_some() || repo.full_name.rsplit('/').next() == Some(NEW_REPO) || account.is_memory(&repo);
    let (prefix, branch) = if own {
        (String::new(), repo.default_branch.clone())
    } else if t.no_branch {
        (FOLDER.to_string(), repo.default_branch.clone())
    } else {
        (FOLDER.to_string(), BRANCH.to_string())
    };
    Ok(config_of(&account, &repo, in_folder(t, prefix), branch, t.env.then_some(true), None))
}

/// In a git repository, the folder of the memory: --folder, else the one
/// its layout gives.
fn in_folder(t: &Target, prefix: String) -> String {
    match &t.folder {
        Some(f) => f.replace('\\', "/").trim_matches('/').to_string(),
        None => prefix,
    }
}

/// `env`: what the user chose now; None takes the memory's setting (off
/// for a new one).
pub fn config_of(
    account: &Account,
    r: &Repo,
    prefix: String,
    branch: String,
    env: Option<bool>,
    files: Option<bool>,
) -> Config {
    // the size limit set before stays when the memory moves
    let before = Config::load_any();
    let (env, env_at) = match env {
        Some(on) => (on, crate::output::now()),
        None => (false, 0),
    };
    Config {
        kind: "github".into(),
        repo: r.full_name.clone(),
        remote: r.clone_url.clone(),
        branch,
        prefix,
        env: env && r.private,
        env_at,
        public: !r.private,
        attachments: files.or(before.as_ref().map(|c| c.attachments)).unwrap_or(false) && r.private,
        attach_mb: before.as_ref().map(|c| c.attach_mb).unwrap_or(crate::sync::ATTACH_MB),
        account: account.login.clone(),
        gh: account.gh.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        connected: crate::output::now(),
        ..Config::default()
    }
}

fn public_ok(c: &Config, t: &Target) -> Result<()> {
    if c.public && !t.yes {
        return Err(fail(
            USAGE,
            format!(
                "{} is public: anyone could read the memory. We do not recommend it; to go on anyway add --yes",
                c.repo
            ),
        ));
    }
    if c.env && c.kind == "git" {
        println!("pg cannot tell whether {} is private: the .env files go only because --env asked for it", c.place());
    }
    Ok(())
}

fn connect(cx: &Context, t: &Target) -> Result<()> {
    let c = config_for(cx, t)?;
    public_ok(&c, t)?;
    for l in sync::transfer::connect(&cx.home, &c, there_of(t)?)? {
        println!("{l}");
    }
    Ok(())
}

/// What is done with a memory found at the place: --there, else joined
/// with --yes, else asked.
fn there_of(t: &Target) -> Result<There> {
    match &t.there {
        Some(w) => {
            There::parse(&w.to_ascii_lowercase()).ok_or_else(|| fail(USAGE, "--there join, update, replace or take"))
        }
        None if t.yes => Ok(There::Join),
        None => Ok(There::Ask),
    }
}

/// Connects to `c` (sync/transfer.rs): a memory already there is joined
/// with this one (when `join`), else this one is sent.
pub fn start(cx: &Context, c: &Config, join: bool) -> Result<Vec<String>> {
    sync::transfer::connect(&cx.home, c, if join { There::Join } else { There::Ask })
}

fn move_to(cx: &Context, t: &Target, remove_old: bool) -> Result<()> {
    let old = need(cx)?;
    let new = config_for(cx, t)?;
    public_ok(&new, t)?;
    for l in sync::transfer::transfer(&cx.home, &old, &new, remove_old, there_of(t)?)? {
        println!("{l}");
    }
    Ok(())
}

/// Moves the memory from `old` to `new`, checked (sync/transfer.rs); a
/// memory found there is joined.
pub fn move_memory(cx: &Context, old: &Config, new: &Config, remove_old: bool) -> Result<Vec<String>> {
    sync::transfer::transfer(&cx.home, old, new, remove_old, There::Join)
}

fn downloads() -> PathBuf {
    std::env::home_dir().unwrap_or_default().join("Downloads")
}

fn backup_cmd(
    cx: &Context,
    folder: Option<PathBuf>,
    env: bool,
    daily: Option<&str>,
    to: Option<PathBuf>,
    keep: Option<usize>,
    daily_now: bool,
) -> Result<()> {
    if daily_now {
        if let Some(p) = backup::daily_now(&cx.home)? {
            println!("ok backed up: {}", p.display());
        }
        return Ok(());
    }
    if let Some(setting) = daily {
        let mut d = backup::Daily::load(&cx.home);
        d.on = match setting.to_ascii_lowercase().as_str() {
            "on" | "yes" | "true" => true,
            "off" | "no" | "false" => false,
            _ => return Err(fail(USAGE, "pg sync backup --daily on --to <folder>, or --daily off")),
        };
        if let Some(f) = to.or(folder) {
            d.folder = std::path::absolute(f)?.to_string_lossy().into_owned();
        }
        if d.on && d.folder.is_empty() {
            return Err(fail(USAGE, "where the daily backups go: --to <folder>"));
        }
        if let Some(k) = keep {
            d.keep = k.clamp(1, 365);
        }
        d.env = env;
        d.save(&cx.home)?;
        if d.on {
            println!(
                "ok daily backup: on, into {} by pg's first command of each day; the last {} kept{}",
                d.folder,
                d.keep,
                if d.env { "; the .env files (keys) go into them" } else { "" }
            );
            backup::daily_check(&cx.home);
        } else {
            println!("ok daily backup: off (the backups already made stay)");
        }
        return Ok(());
    }
    let dir = std::path::absolute(folder.unwrap_or_else(downloads))?;
    let path = dir.join(backup::name(backup::PREFIX, crate::output::now()));
    let m = backup::write(&cx.home, &path, env)?;
    println!(
        "ok backed up: {} ({} of {}, {})",
        path.display(),
        crate::output::count(m.files.len() as i64, "file"),
        crate::output::count(m.projects() as i64, "project"),
        crate::project::attach::size(std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0))
    );
    if env {
        println!("it holds the .env files (keys): keep it somewhere safe");
    }
    Ok(())
}

fn backups(cx: &Context) -> Result<()> {
    let found = backup::found(&cx.home);
    if found.is_empty() {
        println!("no Paragma backups found (in Downloads, Desktop, Documents, the cloud folders): pg sync backup");
        return Ok(());
    }
    println!("ok {} found, newest first:", crate::output::count(found.len() as i64, "backup"));
    for (p, m) in found {
        println!(
            "  {}  from {}  {}, {}  {}",
            crate::output::date_time(m.created),
            if m.host.is_empty() { "?".into() } else { m.host.clone() },
            crate::output::count(m.projects() as i64, "project"),
            crate::output::count(m.files.len() as i64, "file"),
            p.display()
        );
    }
    println!("restore one: pg sync restore <zip> (joined with the memory), --replace (in its place)");
    Ok(())
}

fn restore(cx: &Context, zip: Option<PathBuf>, replace: bool, yes: bool) -> Result<()> {
    let zip = match zip {
        Some(z) => z,
        None => backup::found(&cx.home)
            .into_iter()
            .next()
            .map(|(p, _)| p)
            .ok_or_else(|| fail(USAGE, "no backup found: pg sync restore <zip>"))?,
    };
    if replace && !yes {
        return Err(fail(
            USAGE,
            "replacing makes the memory what the backup holds: what it does not have goes (a copy is kept until it is done). To go on add --yes",
        ));
    }
    let mode = if replace { backup::Mode::Replace } else { backup::Mode::Merge };
    let r = backup::restore(&cx.home, &zip, mode)?;
    println!(
        "ok restored {}: {} written, {} as they were{}{}",
        zip.display(),
        crate::output::count(r.added as i64, "file"),
        r.same,
        if r.removed > 0 { format!(", {} the backup does not have taken out", r.removed) } else { String::new() },
        if r.conflicts.is_empty() {
            String::new()
        } else {
            format!("; {} differ, the backup's kept beside: {}", r.conflicts.len(), r.conflicts.join(", "))
        }
    );
    Ok(())
}
