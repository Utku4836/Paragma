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

/// Connects to `c`: a memory already there is joined with this one (when
/// `join`), else this one is sent. The memory's id is the one there, else
/// the one it had, else a new one. What happened, for people.
pub fn connect(home: &Home, c: &Config, join: bool) -> Result<Vec<String>> {
    let _lock = super::SyncLock::take(home, std::time::Duration::from_secs(120))?;
    let had = Config::load(home);
    let mut c = c.clone();
    if c.id.is_empty() {
        c.id = had.as_ref().map(|h| h.id.clone()).filter(|i| !i.is_empty()).unwrap_or_else(super::new_id);
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
                    if !join && !home.projects().is_empty() {
                        bail!("{} already holds a Paragma memory: to join it with this one add --yes", c.place());
                    }
                    // the memory there is this one from now on
                    if let Some(id) = mark["id"].as_str().filter(|i| !i.is_empty()) {
                        c.id = id.to_string();
                        c.save(home)?;
                    }
                    let m = store::Mirror::new(home, &c, s.as_ref());
                    let o = m.restore()?;
                    out.push(format!(
                        "a memory was there: joined with this one ({} from there, {} from here)",
                        o.got, o.sent
                    ));
                    o
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
                if !join && !home.projects().is_empty() {
                    bail!("{} already holds a Paragma memory: to join it with this one add --yes", c.place());
                }
                if let Some(id) =
                    m.marker().and_then(|v| v["id"].as_str().map(str::to_string)).filter(|i| !i.is_empty())
                {
                    c.id = id;
                    c.save(home)?;
                }
                let m = repo::Mirror::new(home, &c)?;
                let o = m.restore()?;
                out.push(format!(
                    "a memory was there: joined with this one ({} from there, {} from here)",
                    o.got, o.sent
                ));
                o
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
        let _ = forget_place(home);
    }
    result
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
pub fn transfer(home: &Home, old: &Config, new: &Config, remove_old: bool) -> Result<Vec<String>> {
    if old.kind == new.kind && old.remote == new.remote && old.prefix == new.prefix && old.branch == new.branch {
        bail!("the memory is already in {}", old.place());
    }
    let mut out = Vec::new();
    // 1. the copy
    let safety = dir(home).join(backup::name("before-transfer-", crate::output::now()));
    backup::write(home, &safety, true).context("copy the memory before moving it")?;
    out.push("ok copied the memory first".to_string());
    // 2. what is new here to the old place, then the new place
    let _ = super::sync_now(home);
    let mut new = new.clone();
    if new.id.is_empty() {
        new.id = old.id.clone();
    }
    let moved = connect(home, &new, true).and_then(|lines| {
        // 3. the checks
        let now = Config::load(home).context("the new place's settings")?;
        check(home, &now)?;
        Ok((lines, now))
    });
    let (lines, now) = match moved {
        Ok(x) => x,
        Err(e) => {
            // back as it was: the settings, the memory from the copy
            let _ = old.save(home);
            let _ = forget_place(home);
            let back = backup::read_checked(&safety).and_then(|(_, files)| {
                restore_files(home, &files)?;
                Ok(())
            });
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
    out.extend(lines);
    out.push(format!("ok checked: the connection works and every file is in {} as here", now.place()));
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
            "the memory is in {}, but {} could not be told ({e:#}): connect the other computers to the new place by hand",
            now.place(),
            old.place()
        )),
    }
    if remove_old && old.kind == "github" && old.env && old.public {
        out.push("it was public: change the keys that were in its .env files".into());
    }
    Ok(out)
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
            std::fs::remove_file(projects.join(&rel))?;
            repo::remove_empty_dirs(&projects, &rel);
        }
    }
    Ok(())
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/sync/transfer.rs"]
mod tests;
