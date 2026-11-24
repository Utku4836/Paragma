//! Connecting the memory to a place, and moving it from one place to
//! another of any kind (GitHub, another git host, a cloud folder, WebDAV)
//! with nothing lost:
//!
//! 1. the whole memory is copied first (a backup zip in pg's folder);
//! 2. what is new here goes to the old place, then the memory to the new
//!    one (joined with a memory already there);
//! 3. two checks: the connection (a file written there, read back; for git
//!    the branch there is what was pushed) and the content (every file
//!    there, byte for byte as here);
//! 4. both pass: the old place gets a note of where the memory went (other
//!    computers follow it), its files go when asked, and the copy is
//!    deleted. Anything fails: the memory and the settings are taken back
//!    from the copy, the old place untouched, and what failed is said.

use super::{Config, Status, backup, dir, forget_place, repo, store, store_of};
use crate::project::Home;
use anyhow::{Context, Result, bail};

/// What is done when the place already holds a memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum There {
    /// Said, nothing done: the person chooses (when this computer has a
    /// memory; an empty one takes what is there).
    Ask,
    /// Both joined: a note both have keeps both sides' lines.
    Join,
    /// There made what this computer's memory is: an older copy of it
    /// brought up to date, or another memory put aside. Zipped first.
    Replace,
    /// This computer's memory made what is there. Zipped first.
    Take,
}

impl There {
    pub fn parse(word: &str) -> Option<There> {
        Some(match word {
            "join" => There::Join,
            "replace" | "update" => There::Replace,
            "take" => There::Take,
            _ => return None,
        })
    }
}

/// What a place already holds, for this computer's memory: an older copy
/// of it (the same id), or another memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    ThisOne,
    Another,
}

/// Whether the memory found (its id) is this computer's (`ours`).
pub fn found_of(ours: &str, theirs: &str) -> Found {
    if !ours.is_empty() && theirs == ours { Found::ThisOne } else { Found::Another }
}

/// Said when a memory is there and nobody chose what to do.
fn choose_first(place: &str, found: &Found) -> String {
    match found {
        Found::ThisOne => format!(
            "{place} holds an older copy of this memory: --there update makes it the same as this one, --there join joins them, pg sync remove-there takes it out (what changes is zipped first). To move the memory within one place, use pg sync move"
        ),
        Found::Another => format!(
            "{place} already holds another Paragma memory: --there join keeps both, --there replace puts this one in its place, --there take makes this computer's memory that one (what changes is zipped first). To move the memory within one place, use pg sync move"
        ),
    }
}

/// Where the zips made before replacing go: the Downloads folder.
fn zips() -> std::path::PathBuf {
    std::env::home_dir().unwrap_or_default().join("Downloads")
}

/// Connects to `c`: a memory already there is handled as `there` says,
/// else this one is sent. The memory's id is the one there (joined,
/// taken), else the one it had, else a new one. What happened, for people.
pub fn connect(home: &Home, c: &Config, there: There) -> Result<Vec<String>> {
    let _done = super::progress::Done(home);
    let _lock = super::SyncLock::take(home, std::time::Duration::from_secs(120))?;
    let had = Config::load(home);
    // this memory's id, before anything is connected (`c` may carry the
    // id of the memory at the place, to find its folder)
    let ours = super::memory_id(home);
    let mine = if ours.is_empty() { super::new_id() } else { ours.clone() };
    let mut c = c.clone();
    if c.id.is_empty() {
        c.id = mine.clone();
    }
    if had.as_ref().is_none_or(|h| h.remote != c.remote || h.kind != c.kind) {
        forget_place(home)?;
    }
    c.save(home)?;
    let result = (|| -> Result<Vec<String>> {
        let mut out = vec![format!("ok connected: {}{}", c.place(), visibility(&c))];
        let outcome = if c.files() {
            let s = store_of(home, &c)?;
            let m = store::Mirror::new(home, &c, s.as_ref());
            match m.marker() {
                Some(mark) => {
                    let theirs = mark["id"].as_str().unwrap_or("").to_string();
                    let found = found_of(&ours, &theirs);
                    if there == There::Ask && !home.projects().is_empty() {
                        return Err(crate::cli::fail(crate::cli::USAGE, choose_first(&c.place(), &found)));
                    }
                    c.id = id_after(there, &mine, &theirs);
                    c.save(home)?;
                    let m = store::Mirror::new(home, &c, s.as_ref());
                    let ops = Ops {
                        read_all: &|| m.read_all(),
                        replace: &|| m.replace(),
                        take: &|| m.take(),
                        join: &|| m.restore(),
                    };
                    handle(home, &theirs, there, &found, &mut out, ops)?
                }
                None => {
                    let o = m.sync()?;
                    out.push(format!("sent {}", files(o.sent)));
                    o
                }
            }
        } else {
            let m = repo::Mirror::new(home, &c)?;
            if m.memory_at_root() {
                bail!("{} holds a Paragma memory at its root: connect to it as it is", c.place());
            }
            if m.has_memory() {
                let mark = m.marker().unwrap_or_default();
                let theirs = mark["id"].as_str().unwrap_or("").to_string();
                let found = found_of(&ours, &theirs);
                if there == There::Ask && !home.projects().is_empty() {
                    return Err(crate::cli::fail(crate::cli::USAGE, choose_first(&c.place(), &found)));
                }
                c.id = id_after(there, &mine, &theirs);
                c.save(home)?;
                let m = repo::Mirror::new(home, &c)?;
                let ops = Ops {
                    read_all: &|| m.read_all(),
                    replace: &|| m.replace(),
                    take: &|| m.take(),
                    join: &|| m.restore(),
                };
                handle(home, &theirs, there, &found, &mut out, ops)?
            } else {
                let o = m.sync()?;
                out.push(format!("sent {}", files(o.sent)));
                o
            }
        };
        let mut s = Status::load(home);
        s.synced = crate::output::now();
        s.tried = s.synced;
        s.error.clear();
        s.did = outcome.said();
        s.conflicts = outcome.conflicts;
        let _ = s.save(home);
        out.push(format!(".env files: {}", if c.env { "sent" } else { "not sent (pg sync env on)" }));
        if !c.attachments {
            out.push("files kept with notes: not sent (pg sync files on)".into());
        }
        out.push("from now on every change here goes there by itself, and what other computers send comes here".into());
        super::history::add(home, &format!("connected to {}", c.place()), true);
        Ok(out)
    })();
    if result.is_err() {
        // not connected after all: as it was
        match &had {
            Some(h) => h.save(home)?,
            None => {
                let _ = std::fs::remove_file(dir(home).join("config.json"));
            }
        }
        // and the memory's id as it was
        let _ = match ours.is_empty() {
            true => std::fs::remove_file(dir(home).join("id")),
            false => std::fs::write(dir(home).join("id"), &ours),
        };
        let _ = forget_place(home);
    }
    result
}

/// What can be done with a memory found at a place.
struct Ops<'a> {
    read_all: &'a dyn Fn() -> Result<Vec<(String, Vec<u8>)>>,
    replace: &'a dyn Fn() -> Result<repo::Outcome>,
    take: &'a dyn Fn() -> Result<repo::Outcome>,
    join: &'a dyn Fn() -> Result<repo::Outcome>,
}

/// The memory's id once a memory found at a place is handled: this one's
/// when it is put there, else the one there (joined, taken).
fn id_after(there: There, mine: &str, theirs: &str) -> String {
    if there == There::Replace || theirs.is_empty() { mine.to_string() } else { theirs.to_string() }
}

/// A memory found there (`theirs`, its id), handled as `there` says: what
/// changes zipped first into Downloads, then joined, replaced or taken.
fn handle(
    home: &Home,
    theirs: &str,
    there: There,
    found: &Found,
    out: &mut Vec<String>,
    ops: Ops,
) -> Result<repo::Outcome> {
    let what = if *found == Found::ThisOne { "an older copy of this memory" } else { "a memory" };
    let now = crate::output::now();
    Ok(match there {
        There::Replace => {
            let zip = zips().join(backup::name("paragma-backup-there-", now));
            backup::write_files(&zip, theirs, (ops.read_all)()?).context("zip what was there before replacing it")?;
            out.push(format!("ok zipped what was there first: {}", zip.display()));
            let o = (ops.replace)()?;
            out.push(format!("{what} was there: made the same as this one ({} sent)", files(o.sent)));
            o
        }
        There::Take => {
            let zip = zips().join(backup::name("paragma-backup-here-", now));
            backup::write(home, &zip, true).context("zip this computer's memory before replacing it")?;
            out.push(format!("ok zipped this computer's memory first: {}", zip.display()));
            let o = (ops.take)()?;
            out.push(format!("{what} was there: this computer's memory made the same ({} changed here)", files(o.got)));
            o
        }
        There::Join | There::Ask => {
            let o = (ops.join)()?;
            out.push(format!("{what} was there: joined with this one ({} from there, {} from here)", o.got, o.sent));
            o
        }
    })
}

/// The memory in `c` taken out (zipped first into Downloads); nothing is
/// connected. What happened, for people.
pub fn remove_there(home: &Home, c: &Config) -> Result<Vec<String>> {
    let _lock = super::SyncLock::take(home, std::time::Duration::from_secs(120))?;
    let zip = zips().join(backup::name("paragma-backup-there-", crate::output::now()));
    let mut out = Vec::new();
    if c.files() {
        let s = store_of(home, c)?;
        let m = store::Mirror::new(home, c, s.as_ref());
        if m.marker().is_none() {
            bail!("{} holds no Paragma memory", c.place());
        }
        backup::write_files(&zip, &c.id, m.read_all()?).context("zip what is there before taking it out")?;
        out.push(format!("ok zipped it first: {}", zip.display()));
        m.remove_all()?;
    } else {
        let dir = dir(home).join("old-checkout");
        let _ = super::remove_clone(&dir);
        let r = (|| -> Result<()> {
            let m = repo::Mirror::at(home, c, dir.clone())?;
            if !m.has_memory() {
                bail!("{} holds no Paragma memory", c.place());
            }
            backup::write_files(&zip, &c.id, m.read_all()?).context("zip what is there before taking it out")?;
            out.push(format!("ok zipped it first: {}", zip.display()));
            m.remove_all()
        })();
        let _ = super::remove_clone(&dir);
        r?;
    }
    out.push(format!("ok the memory in {} is taken out; nothing was connected", c.place()));
    super::history::add(home, &format!("took the memory out of {} (zipped first)", c.place()), true);
    Ok(out)
}

fn files(n: usize) -> String {
    if n == 1 { "1 file".into() } else { format!("{n} files") }
}

fn visibility(c: &Config) -> &'static str {
    match c.kind.as_str() {
        "github" if c.public => " (public)",
        "github" => " (private)",
        _ => "",
    }
}

/// The two checks of a place, after connecting: the connection, and every
/// file there as here. What failed, for people.
pub fn check(home: &Home, c: &Config) -> Result<()> {
    let wrong = if c.files() {
        let s = store_of(home, c)?;
        let m = store::Mirror::new(home, c, s.as_ref());
        m.probe().context("the connection check")?;
        m.verify()?
    } else {
        let m = repo::Mirror::new(home, c)?;
        m.probe().context("the connection check")?;
        m.verify()?
    };
    if !wrong.is_empty() {
        let shown: Vec<&str> = wrong.iter().take(5).map(String::as_str).collect();
        bail!(
            "{} file(s) are not in {} as they are here: {}{}",
            wrong.len(),
            c.place(),
            shown.join(", "),
            if wrong.len() > 5 { ", …" } else { "" }
        );
    }
    Ok(())
}

/// Moves the memory from `old` to `new`, checked, as the module says.
pub fn transfer(home: &Home, old: &Config, new: &Config, remove_old: bool, there: There) -> Result<Vec<String>> {
    if old.kind == new.kind && old.remote == new.remote && old.prefix == new.prefix && old.branch == new.branch {
        bail!("the memory is already in {}", old.place());
    }
    let _done = super::progress::Done(home);
    let mut out = Vec::new();
    // 1. the copy
    super::progress::set(home, "copying the memory first", 0, 0);
    let safety = dir(home).join(backup::name("before-transfer-", crate::output::now()));
    backup::write(home, &safety, true).context("copy the memory before moving it")?;
    out.push("ok copied the memory first".to_string());
    // 2. what is new here to the old place, then the new place
    let _ = super::sync_now(home);
    let mut new = new.clone();
    if new.id.is_empty() {
        new.id = old.id.clone();
    }
    // what this computer knows of the old place, kept for going back
    let kept = keep_base(home);
    let moved = connect(home, &new, there).and_then(|lines| {
        // 3. the checks
        let now = Config::load(home).context("the new place's settings")?;
        check(home, &now)?;
        Ok((lines, now))
    });
    let (lines, now) = match moved {
        Ok(x) => x,
        Err(e) => {
            // back as it was: the settings, what it knew of the old place,
            // the memory from the copy
            let _ = old.save(home);
            let _ = forget_place(home);
            put_base_back(home, &kept);
            let back = backup::read_checked(&safety).and_then(|(_, files)| {
                restore_files(home, &files)?;
                Ok(())
            });
            super::history::add(
                home,
                &format!("moving to {} failed: {}", new.place(), crate::output::error_text(&e)),
                false,
            );
            return match back {
                Ok(()) => {
                    let _ = std::fs::remove_file(&safety);
                    Err(e.context(format!("the memory was not moved; it stays in {} as before", old.place())))
                }
                Err(_) => {
                    Err(e.context(format!("the memory was not moved; its copy from before is in {}", safety.display())))
                }
            };
        }
    };
    drop_kept(&kept);
    out.extend(lines);
    out.push(format!("ok checked: the connection works and every file is in {} as here", now.place()));
    super::history::add(home, &format!("moved from {} to {}, checked", old.place(), now.place()), true);
    // 4. the note in the old place, the copy deleted
    let noted = leave_note(home, old, &now, remove_old);
    let _ = std::fs::remove_file(&safety);
    match noted {
        Ok(()) => out.push(format!(
            "{} says where the memory went: other computers follow by themselves{}",
            old.place(),
            if remove_old { "; the memory's files left it" } else { "" }
        )),
        Err(e) => out.push(format!(
            "the memory is in {}, but {} could not be told ({}): connect the other computers to the new place by hand",
            now.place(),
            old.place(),
            crate::output::error_text(&e)
        )),
    }
    if remove_old && old.kind == "github" && old.env && old.public {
        out.push("it was public: change the keys that were in its .env files".into());
    }
    Ok(out)
}

/// The base of a place of files (sync/store.rs), moved aside while
/// another place is tried: its manifest and copies.
fn keep_base(home: &Home) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    let d = dir(home);
    let mut kept = Vec::new();
    for name in ["base.json", "base"] {
        let (from, to) = (d.join(name), d.join(format!("{name}.before-transfer")));
        let _ = std::fs::remove_file(&to);
        let _ = std::fs::remove_dir_all(&to);
        if from.exists() && std::fs::rename(&from, &to).is_ok() {
            kept.push((from, to));
        }
    }
    kept
}

fn put_base_back(_home: &Home, kept: &[(std::path::PathBuf, std::path::PathBuf)]) {
    for (from, to) in kept {
        let _ = std::fs::rename(to, from);
    }
}

fn drop_kept(kept: &[(std::path::PathBuf, std::path::PathBuf)]) {
    for (_, to) in kept {
        let _ = std::fs::remove_file(to);
        let _ = std::fs::remove_dir_all(to);
    }
}

/// The old place's note of where the memory went (and its files out, when
/// asked). A git place through a clone of its own, so pg's usual clone
/// stays the new place's.
fn leave_note(home: &Home, old: &Config, new: &Config, remove: bool) -> Result<()> {
    if old.files() {
        let s = store_of(home, old)?;
        return store::Mirror::new(home, old, s.as_ref()).moved(new.place_json(), remove);
    }
    let dir = dir(home).join("old-checkout");
    let _ = super::remove_clone(&dir);
    let r = repo::Mirror::at(home, old, dir.clone()).and_then(|m| m.moved(new, remove));
    let _ = super::remove_clone(&dir);
    r
}

/// The memory made what a backup holds (the files of the projects only).
fn restore_files(home: &Home, files: &[(String, Vec<u8>)]) -> Result<()> {
    let projects = home.projects_dir();
    let names: std::collections::HashSet<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    for (rel, bytes) in files {
        crate::project::files::write_atomic(&projects.join(rel), bytes)?;
    }
    let mut now = Vec::new();
    repo::walk(&projects, "", &mut now, &|rel, is_dir| repo::kept(rel, is_dir, true))?;
    for rel in now {
        if !names.contains(rel.as_str()) {
            crate::sync::fsx::remove_file(projects.join(&rel))?;
            repo::remove_empty_dirs(&projects, &rel);
        }
    }
    Ok(())
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/sync/transfer.rs"]
mod tests;
