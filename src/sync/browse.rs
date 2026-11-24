//! Going through the folders of a place to choose where the memory goes:
//! this computer's (a cloud's folder, any folder), a WebDAV server's, a git
//! repository's (its newest commit, fetched without the files' contents).
//! A folder is said by what it holds: nothing, a Paragma memory (its id),
//! or other files (the memory needs a folder of its own).

use super::MARKER;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// Where the folders are.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// This computer: `at` is a full path; "" is the computer itself (its
    /// disks).
    Local,
    /// A WebDAV server: `at` is a path below its address.
    Dav { url: String, user: String, password: String },
    /// A git repository: `at` is a path in it. `dir` holds what was fetched;
    /// `rev` is the commit looked at; `gh`, the GitHub CLI that signs in.
    Git { dir: PathBuf, rev: String, gh: String },
}

/// What a folder holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Holds {
    Nothing,
    /// A Paragma memory, by its id.
    Memory(String),
    /// Other files: no place for the memory.
    Other,
}

/// A folder's folders, and what it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Listing {
    pub dirs: Vec<String>,
    pub holds: Holds,
}

/// The folder below `at`.
pub fn join(source: &Source, at: &str, name: &str) -> String {
    match source {
        Source::Local if at.is_empty() => name.to_string(),
        Source::Local => Path::new(at).join(name).to_string_lossy().into_owned(),
        _ if at.is_empty() => name.to_string(),
        _ => format!("{}/{name}", at.trim_end_matches('/')),
    }
}

/// The folder above `at`; None at the top.
pub fn parent(source: &Source, at: &str) -> Option<String> {
    if at.is_empty() {
        return None;
    }
    match source {
        Source::Local => Some(Path::new(at).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()),
        _ => Some(at.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default()),
    }
}

/// The folders in `at` and what it holds.
pub fn list(source: &Source, scratch: &Path, at: &str) -> Result<Listing> {
    match source {
        Source::Local => local(at),
        Source::Dav { url, user, password } => {
            let dav = super::webdav::WebDav::new(url, user, password, scratch.to_path_buf());
            let (dirs, files) = dav.entries(at)?;
            let holds = if files.iter().any(|f| f == MARKER) {
                let rel = join(source, at, MARKER);
                Holds::Memory(id_in(&super::store::Store::read(&dav, &rel).unwrap_or_default()))
            } else if dirs.is_empty() && files.is_empty() {
                Holds::Nothing
            } else {
                Holds::Other
            };
            Ok(Listing { dirs, holds })
        }
        Source::Git { dir, rev, gh } => {
            let tree = if at.is_empty() { rev.clone() } else { format!("{rev}:{at}") };
            let names = git(dir, gh, &["ls-tree", "--name-only", &tree]).unwrap_or_default();
            let dirs: Vec<String> = git(dir, gh, &["ls-tree", "--name-only", "-d", &tree])
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .filter(|n| !n.is_empty())
                .collect();
            let holds = if names.lines().any(|n| n == MARKER) {
                let file = format!("{rev}:{}", join(source, at, MARKER));
                Holds::Memory(id_in(git(dir, gh, &["show", &file]).unwrap_or_default().as_bytes()))
            } else if names.trim().is_empty() {
                Holds::Nothing
            } else {
                Holds::Other
            };
            Ok(Listing { dirs: sorted(dirs), holds })
        }
    }
}

/// A new folder `name` in `at`; where it is. In a git repository it is
/// made with the memory's first files.
pub fn make(source: &Source, scratch: &Path, at: &str, name: &str) -> Result<String> {
    let name = name.trim().trim_matches(['/', '\\']);
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        bail!("a folder's name, without / or \\");
    }
    let to = join(source, at, name);
    match source {
        Source::Local => {
            if at.is_empty() {
                bail!("choose a disk first");
            }
            super::fsx::create_dir_all(&to)?;
        }
        Source::Dav { url, user, password } => {
            super::webdav::WebDav::new(url, user, password, scratch.to_path_buf()).make_folder(&to)?;
        }
        Source::Git { .. } => {}
    }
    Ok(to)
}

fn local(at: &str) -> Result<Listing> {
    if at.is_empty() {
        return Ok(Listing { dirs: disks(), holds: Holds::Other });
    }
    let p = Path::new(at);
    let entries = std::fs::read_dir(p).with_context(|| format!("list the folder {at}"))?;
    let mut dirs = Vec::new();
    let mut other = false;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        // the system's own file and pg's say nothing of what is there
        if name.eq_ignore_ascii_case("desktop.ini") || name == MARKER || name.starts_with(".paragma-probe") {
            continue;
        }
        if is_dir && !name.starts_with('.') && !name.starts_with('$') {
            dirs.push(name);
        } else {
            other = true;
        }
    }
    let holds = match super::cloud::marker_id(p) {
        Some(id) => Holds::Memory(id),
        None if other || !dirs.is_empty() => Holds::Other,
        None => Holds::Nothing,
    };
    Ok(Listing { dirs: sorted(dirs), holds })
}

/// The disks of this computer (Windows), or its root.
fn disks() -> Vec<String> {
    if !cfg!(windows) {
        return vec!["/".into()];
    }
    (b'A'..=b'Z').map(|l| format!("{}:\\", l as char)).filter(|d| Path::new(d).is_dir()).collect()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort_by_key(|s| s.to_lowercase());
    v
}

/// A marker's id.
fn id_in(bytes: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|v| v["id"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn git(dir: &Path, gh: &str, args: &[&str]) -> Result<String> {
    let mut c = std::process::Command::new("git");
    if !gh.is_empty() {
        let helper = format!("credential.helper=!'{}' auth git-credential", gh.replace('\\', "/"));
        c.args(["-c", "credential.helper=", "-c", &helper]);
    }
    let out = c
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .output()
        .context("run git (is it installed?)")?;
    if !out.status.success() {
        bail!("git {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A git repository looked at: its newest commit fetched (no files'
/// contents) into `dir`. None for an empty repository (the memory goes at
/// its root); its default branch too.
pub fn open_git(url: &str, dir: &Path, gh: &str) -> Result<Option<(Source, String)>> {
    let _ = super::remove_clone(dir);
    super::fsx::create_dir_all(dir)?;
    let heads = git(dir, gh, &["ls-remote", "--symref", url]).with_context(|| format!("reach {url}"))?;
    if heads.trim().is_empty() {
        return Ok(None);
    }
    let branch = heads
        .lines()
        .find_map(|l| l.strip_prefix("ref: refs/heads/").and_then(|r| r.split_whitespace().next()))
        .unwrap_or("main")
        .to_string();
    git(dir, gh, &["init", "-q"])?;
    git(dir, gh, &["remote", "add", "origin", url])?;
    // a file's content (a marker) brought when it is read
    git(dir, gh, &["config", "remote.origin.promisor", "true"])?;
    git(dir, gh, &["config", "remote.origin.partialclonefilter", "blob:none"])?;
    git(dir, gh, &["fetch", "-q", "--depth", "1", "--filter=blob:none", "origin", &branch])?;
    let rev = git(dir, gh, &["rev-parse", "FETCH_HEAD"])?.trim().to_string();
    Ok(Some((Source::Git { dir: dir.to_path_buf(), rev, gh: gh.to_string() }, branch)))
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/sync/browse.rs"]
mod tests;
