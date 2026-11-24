//! The memory kept in a place of files: a cloud folder on this computer
//! (OneDrive, Google Drive, Dropbox, iCloud: their own program carries it)
//! or a WebDAV server (Nextcloud and others). One sync logic for both,
//! over a `Store` that lists, reads, writes and removes files.
//!
//! Each sync compares three versions of every file: here (the projects),
//! there (the place), and the base (both as they were at the last sync,
//! kept in `<home>/git/base/`). A file changed on one side goes to the
//! other; changed on both, a note's lines are joined (git merge-file with
//! the base), and where both changed the same lines this computer's stays
//! and the other one is kept beside it as `name.conflict.md`, as with git.
//! The copies a cloud program makes when two computers changed a file at
//! once (OneDrive's `name-COMPUTER.md`, Dropbox's `name (… conflicted copy
//! 2026-01-02).md`, Google Drive's `name (1).md`, iCloud's `name 2.md`)
//! become such a `.conflict.md` too. Every file written there is read back
//! and compared.

use super::repo::{conflict_name, fingerprint, kept, remove_empty_dirs, union, walk};
use super::{Config, MARKER};
use crate::project::Home;
use crate::project::files::{WriteLock, write_atomic};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A file there: its path with '/', a stamp that changes when the file
/// does (its time and size, or its ETag), and whether it is whole (a file
/// a cloud program is still bringing in is left for the next sync).
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub rel: String,
    pub stamp: String,
    pub settled: bool,
}

/// A place that holds files.
pub trait Store {
    /// Every file below the root.
    fn list(&self) -> Result<Vec<Listed>>;
    fn read(&self, rel: &str) -> Result<Vec<u8>>;
    /// Writes the file whole (never half), reads it back to check it, and
    /// says its stamp there now.
    fn write(&self, rel: &str, bytes: &[u8]) -> Result<String>;
    fn remove(&self, rel: &str) -> Result<()>;
    /// Where it is, for people.
    fn place(&self) -> String;
}

/// A folder on this computer.
pub struct Folder {
    pub root: PathBuf,
    /// A file changed this recently may still be arriving: left for the
    /// next sync.
    pub settle: std::time::Duration,
}

/// The name a file is written under before it takes its own.
const TEMP: &str = ".~pg-";

impl Folder {
    pub fn new(root: PathBuf) -> Folder {
        Folder { root, settle: std::time::Duration::from_secs(2) }
    }
}

impl Store for Folder {
    fn list(&self) -> Result<Vec<Listed>> {
        if !self.root.is_dir() {
            bail!("{} is not there (is the cloud's program running?)", self.root.display());
        }
        let mut files = Vec::new();
        walk(&self.root, "", &mut files, &|rel, _| !rel.rsplit('/').next().unwrap_or(rel).starts_with(TEMP))?;
        let now = std::time::SystemTime::now();
        let mut out = Vec::with_capacity(files.len());
        for rel in files {
            let Ok(m) = std::fs::metadata(self.root.join(&rel)) else { continue };
            let at = m.modified().unwrap_or(std::time::UNIX_EPOCH);
            let settled = now.duration_since(at).unwrap_or_default() >= self.settle;
            out.push(Listed { rel, stamp: stamp(&m), settled });
        }
        Ok(out)
    }

    fn read(&self, rel: &str) -> Result<Vec<u8>> {
        std::fs::read(self.root.join(rel)).with_context(|| format!("read {rel} in {}", self.root.display()))
    }

    fn write(&self, rel: &str, bytes: &[u8]) -> Result<String> {
        let to = self.root.join(rel);
        let dir = to.parent().context("a file's folder")?;
        crate::sync::fsx::create_dir_all(dir)?;
        let name = to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let tmp = dir.join(format!("{TEMP}{}-{name}", std::process::id()));
        std::fs::write(&tmp, bytes).with_context(|| format!("write {rel} in {}", self.root.display()))?;
        if let Err(e) = std::fs::rename(&tmp, &to) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e).with_context(|| format!("put {rel} in place in {}", self.root.display()));
        }
        if std::fs::read(&to).ok().as_deref() != Some(bytes) {
            bail!("{rel} in {} reads back other than written", self.root.display());
        }
        Ok(stamp(&crate::sync::fsx::metadata(&to)?))
    }

    fn remove(&self, rel: &str) -> Result<()> {
        match std::fs::remove_file(self.root.join(rel)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("remove {rel} in {}", self.root.display())),
        }
        remove_empty_dirs(&self.root, rel);
        Ok(())
    }

    fn place(&self) -> String {
        self.root.display().to_string()
    }
}

/// A file's stamp in a folder: its time and size.
fn stamp(m: &std::fs::Metadata) -> String {
    let ns =
        m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{ns}-{}", m.len())
}

/// What a sync did.
pub use super::repo::Outcome;

/// More of the files than this share gone from one side at once, of more
/// than `MASS` files, is not taken as deleted: a cloud folder emptied by
/// its program, a disk not there, are said instead.
const MASS_SHARE: f64 = 0.5;
const MASS: usize = 10;

/// A file as it was at the last sync: its bytes' fingerprint here and
/// there, and its stamp there.
#[derive(Clone, Debug, PartialEq)]
struct Was {
    hash: u64,
    stamp: String,
}

/// One sync between the projects and a store.
pub struct Mirror<'a> {
    home: &'a Home,
    config: &'a Config,
    store: &'a dyn Store,
    /// pg's copy of each file as it was at the last sync, for joining.
    base_dir: PathBuf,
    env: std::cell::Cell<bool>,
}

impl<'a> Mirror<'a> {
    pub fn new(home: &'a Home, config: &'a Config, store: &'a dyn Store) -> Mirror<'a> {
        Mirror {
            home,
            config,
            store,
            base_dir: super::dir(home).join("base"),
            env: std::cell::Cell::new(config.env && !config.public),
        }
    }

    fn manifest_path(&self) -> PathBuf {
        super::dir(self.home).join("base.json")
    }

    fn load_base(&self) -> BTreeMap<String, Was> {
        let Some(v) =
            std::fs::read_to_string(self.manifest_path()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            return BTreeMap::new();
        };
        // a base of another place says nothing about this one
        if v["place"].as_str() != Some(self.config.remote.as_str()) {
            return BTreeMap::new();
        }
        v["files"]
            .as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(k, w)| {
                        Some((
                            k.clone(),
                            Was {
                                hash: u64::from_str_radix(w["hash"].as_str()?, 16).ok()?,
                                stamp: w["stamp"].as_str().unwrap_or("").to_string(),
                            },
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn save_base(&self, base: &BTreeMap<String, Was>) -> Result<()> {
        let files: serde_json::Map<String, Value> = base
            .iter()
            .map(|(k, w)| (k.clone(), json!({ "hash": format!("{:016x}", w.hash), "stamp": w.stamp })))
            .collect();
        let v = json!({ "place": self.config.remote, "files": files });
        write_atomic(&self.manifest_path(), serde_json::to_string(&v)?.as_bytes())
    }

    /// A file's content at the last sync, kept for joining notes.
    fn base_text(&self, rel: &str) -> Option<Vec<u8>> {
        std::fs::read(self.base_dir.join(rel)).ok()
    }

    fn keep_base_text(&self, rel: &str, bytes: Option<&[u8]>) {
        let p = self.base_dir.join(rel);
        match bytes {
            Some(b) if is_text(rel) => {
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::write(&p, b);
            }
            _ => {
                let _ = std::fs::remove_file(&p);
            }
        }
    }

    // ---------- the marker: the memory is here, which one, moved ----------

    pub fn marker(&self) -> Option<Value> {
        let b = self.store.read(MARKER).ok()?;
        serde_json::from_slice(&b).ok()
    }

    fn mark(&self, moved_to: Option<Value>) -> Result<()> {
        let mut v = json!({ "paragma": 1, "id": self.config.id, "env": self.env.get(), "env_at": self.config.env_at });
        if let Some(m) = moved_to {
            v["moved_to"] = m;
        }
        self.store.write(MARKER, (serde_json::to_string_pretty(&v)? + "\n").as_bytes()).map(|_| ())
    }

    /// The .env setting changed last, on any computer, counts.
    fn settle_env(&self, m: &Value) -> Result<()> {
        let (theirs, at) = (m["env"].as_bool().unwrap_or(false), m["env_at"].as_i64().unwrap_or(0));
        if at > self.config.env_at {
            let env = theirs && !self.config.public;
            self.env.set(env);
            let mut c = self.config.clone();
            c.env = env;
            c.env_at = at;
            c.save(self.home)?;
        } else if theirs != self.env.get() || at < self.config.env_at {
            let moved = m.get("moved_to").cloned().filter(|v| !v.is_null());
            self.mark(moved)?;
        }
        Ok(())
    }

    // ---------- what goes ----------

    /// The files of the projects that go, by their path under projects/.
    fn local_files(&self) -> Result<Vec<String>> {
        let held = self.held();
        let mut out = Vec::new();
        walk(&self.home.projects_dir(), "", &mut out, &|rel, is_dir| {
            kept(rel, is_dir, self.env.get()) && (is_dir || !held.contains(rel))
        })?;
        Ok(out)
    }

    /// Files kept with notes that stay on this computer (not sent, or over
    /// the limit): neither sent nor taken out of the place.
    fn held(&self) -> std::collections::HashSet<String> {
        let send = self.config.attachments && !self.config.public;
        let most = self.config.attach_mb * 1024 * 1024;
        crate::project::attach::all(&self.home.projects_dir())
            .into_iter()
            .filter(|(_, size)| !send || *size > most)
            .map(|(rel, _)| rel)
            .collect()
    }

    // ---------- one sync ----------

    pub fn sync(&self) -> Result<Outcome> {
        let mut out = Outcome::default();
        let mut base = self.load_base();
        match self.marker() {
            Some(m) => {
                if let Some(to) = m.get("moved_to").filter(|v| !v.is_null()) {
                    out.moved_to = Some(to.to_string());
                    return Ok(out);
                }
                self.settle_env(&m)?;
            }
            // synced here before, and the memory is not there now: never
            // taken as everything deleted
            None if !base.is_empty() => bail!(
                "the memory is not in {} now (taken out, or its cloud program still bringing it in?): nothing was changed here",
                self.store.place()
            ),
            None => self.mark(None)?,
        }
        let listed = self.store.list()?;
        let busy: BTreeSet<String> = listed.iter().filter(|l| !l.settled).map(|l| l.rel.clone()).collect();
        let mut there: BTreeMap<String, String> = listed
            .into_iter()
            .filter(|l| {
                l.settled
                    && l.rel != MARKER
                    && !l.rel.starts_with(".paragma-probe")
                    && kept(&l.rel, false, true)
                    && !l.rel.split('/').any(|p| p == ".pg" || p == ".git")
            })
            .map(|l| (l.rel, l.stamp))
            .collect();
        let projects = self.home.projects_dir();
        let here: BTreeSet<String> = self.local_files()?.into_iter().collect();
        let held = self.held();
        // most of the files gone from one side at once: said, not followed
        let gone_there = base.keys().filter(|r| !there.contains_key(*r) && !busy.contains(*r)).count();
        let gone_here = base.keys().filter(|r| !here.contains(*r) && !held.contains(*r)).count();
        for (gone, side) in [(gone_there, self.store.place()), (gone_here, "this computer".to_string())] {
            if base.len() > MASS && gone as f64 > base.len() as f64 * MASS_SHARE {
                bail!(
                    "{gone} of the memory's {} files are gone from {side} since the last sync: not followed (pg sync restore brings a backup back; pg sync disconnect stops syncing)",
                    base.len()
                );
            }
        }
        // a cloud program's copies of a file two computers changed at once
        for (copy, of) in client_copies(&there, &base) {
            let bytes = self.store.read(&copy)?;
            let side = free_name(&conflict_name(&of), &there);
            let s = self.store.write(&side, &bytes)?;
            self.store.remove(&copy)?;
            there.remove(&copy);
            there.insert(side.clone(), s);
            out.conflicts.push(side);
        }
        let mut all: BTreeSet<String> = here.clone();
        all.extend(there.keys().cloned());
        all.extend(base.keys().cloned());
        all.retain(|r| !busy.contains(r));
        // one project at a time, pg's own writers there waiting meanwhile
        let mut by_project: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for rel in all {
            if held.contains(&rel) || !kept(&rel, false, self.env.get()) {
                continue;
            }
            by_project.entry(rel.split('/').next().unwrap_or("").to_string()).or_default().push(rel);
        }
        for (project, files) in by_project {
            let _lock = if project.starts_with('.') || project.is_empty() || !files.iter().all(|f| f.contains('/')) {
                None
            } else {
                crate::sync::fsx::create_dir_all(projects.join(&project).join(".pg"))?;
                Some(WriteLock::take(&projects.join(&project).join(".pg"))?)
            };
            for rel in files {
                self.one(&rel, &projects, &mut base, &mut there, &mut out)?;
            }
        }
        self.save_base(&base)?;
        Ok(out)
    }

    /// One file, as the three versions say.
    fn one(
        &self,
        rel: &str,
        projects: &Path,
        base: &mut BTreeMap<String, Was>,
        there: &mut BTreeMap<String, String>,
        out: &mut Outcome,
    ) -> Result<()> {
        let local = std::fs::read(projects.join(rel)).ok();
        let was = base.get(rel).cloned();
        let stamp = there.get(rel).cloned();
        let here_changed = local.as_ref().map(|b| fingerprint(b)) != was.as_ref().map(|w| w.hash);
        let there_changed = match (&stamp, &was) {
            (Some(s), Some(w)) => *s != w.stamp,
            (None, None) => false,
            _ => true,
        };
        let put =
            |bytes: &[u8], base: &mut BTreeMap<String, Was>, there: &mut BTreeMap<String, String>| -> Result<()> {
                let s = self.store.write(rel, bytes)?;
                there.insert(rel.to_string(), s.clone());
                base.insert(rel.to_string(), Was { hash: fingerprint(bytes), stamp: s });
                self.keep_base_text(rel, Some(bytes));
                Ok(())
            };
        let take = |bytes: &[u8], stamp: &str, base: &mut BTreeMap<String, Was>| -> Result<()> {
            write_atomic(&projects.join(rel), bytes)?;
            base.insert(rel.to_string(), Was { hash: fingerprint(bytes), stamp: stamp.to_string() });
            self.keep_base_text(rel, Some(bytes));
            Ok(())
        };
        match (here_changed, there_changed) {
            (false, false) => {}
            (true, false) => match &local {
                Some(b) => {
                    put(b, base, there)?;
                    out.sent += 1;
                }
                None => {
                    if stamp.is_some() {
                        self.store.remove(rel)?;
                        there.remove(rel);
                        out.sent += 1;
                    }
                    base.remove(rel);
                    self.keep_base_text(rel, None);
                }
            },
            (false, true) => match &stamp {
                Some(s) => {
                    let bytes = self.store.read(rel)?;
                    take(&bytes, s, base)?;
                    out.got += 1;
                }
                None => {
                    if local.is_some() {
                        crate::sync::fsx::remove_gone(projects.join(rel))?;
                        remove_empty_dirs(projects, rel);
                        out.got += 1;
                    }
                    base.remove(rel);
                    self.keep_base_text(rel, None);
                }
            },
            (true, true) => {
                let theirs = match &stamp {
                    Some(_) => Some(self.store.read(rel)?),
                    None => None,
                };
                match (&local, &theirs) {
                    (Some(a), Some(b)) if a == b => {
                        let s = stamp.clone().unwrap_or_default();
                        base.insert(rel.to_string(), Was { hash: fingerprint(a), stamp: s });
                        self.keep_base_text(rel, Some(a));
                    }
                    // gone on one side, changed on the other: the change stays
                    (Some(a), None) => {
                        put(a, base, there)?;
                        out.sent += 1;
                    }
                    (None, Some(b)) => {
                        take(b, stamp.as_deref().unwrap_or(""), base)?;
                        out.got += 1;
                    }
                    (None, None) => {
                        base.remove(rel);
                        self.keep_base_text(rel, None);
                    }
                    (Some(a), Some(b)) => {
                        let joined = if is_text(rel) { self.join(rel, a, b)? } else { None };
                        match joined {
                            Some(j) => {
                                write_atomic(&projects.join(rel), &j)?;
                                put(&j, base, there)?;
                                out.sent += 1;
                                out.got += 1;
                            }
                            None => {
                                // this computer's stays; theirs beside it, on both sides
                                let side = free_name(&conflict_name(rel), there);
                                let s = self.store.write(&side, b)?;
                                write_atomic(&projects.join(&side), b)?;
                                base.insert(side.clone(), Was { hash: fingerprint(b), stamp: s.clone() });
                                there.insert(side.clone(), s);
                                put(a, base, there)?;
                                out.conflicts.push(side);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Both sides' changes to a note joined: with the base, git merge-file
    /// (None when both changed the same lines); without one (two memories
    /// joined for the first time), both sides' lines kept.
    fn join(&self, rel: &str, here: &[u8], there: &[u8]) -> Result<Option<Vec<u8>>> {
        let scratch = super::dir(self.home);
        let Some(base) = self.base_text(rel) else { return union(here, there, &scratch).map(Some) };
        let dir = scratch.join("join");
        crate::sync::fsx::create_dir_all(&dir)?;
        let (a, o, b) = (dir.join("here"), dir.join("base"), dir.join("there"));
        crate::sync::fsx::write(&a, here)?;
        crate::sync::fsx::write(&o, &base)?;
        crate::sync::fsx::write(&b, there)?;
        let run = std::process::Command::new("git")
            .args(["merge-file", "-p"])
            .args([&a, &o, &b])
            .output()
            .context("run git merge-file (is git installed?)")?;
        let _ = std::fs::remove_dir_all(&dir);
        Ok((run.status.code() == Some(0)).then_some(run.stdout))
    }

    /// Joining a place that already holds a memory: the first sync with
    /// no base joins both sides (a note both have keeps both sides' lines).
    pub fn restore(&self) -> Result<Outcome> {
        let _ = std::fs::remove_file(self.manifest_path());
        let _ = std::fs::remove_dir_all(&self.base_dir);
        // connected to on purpose: a note that the memory left is taken out
        if self.marker().is_some_and(|m| m.get("moved_to").is_some_and(|v| !v.is_null())) {
            self.mark(None)?;
        }
        self.sync()
    }

    /// Leaves a note there that the memory moved, so other computers
    /// follow; with `remove`, the memory's files go from it.
    pub fn moved(&self, to: Value, remove: bool) -> Result<()> {
        if remove {
            for l in self.store.list()? {
                if l.rel != MARKER {
                    self.store.remove(&l.rel)?;
                }
            }
        }
        self.mark(Some(to))
    }

    /// The connection works: a probe written, read back, taken out.
    pub fn probe(&self) -> Result<()> {
        let name = format!(".paragma-probe-{}", std::process::id());
        let bytes = format!("paragma {}", crate::output::now()).into_bytes();
        self.store.write(&name, &bytes)?;
        let back = self.store.read(&name)?;
        self.store.remove(&name)?;
        if back != bytes {
            bail!("a test file read back other than written in {}", self.store.place());
        }
        Ok(())
    }

    /// Every file that goes is there, byte for byte: what differs.
    pub fn verify(&self) -> Result<Vec<String>> {
        let projects = self.home.projects_dir();
        let mut wrong = Vec::new();
        let there: std::collections::HashSet<String> = self.store.list()?.into_iter().map(|l| l.rel).collect();
        for rel in self.local_files()? {
            // a file taken out here meanwhile is not one that went wrong
            let Ok(here) = std::fs::read(projects.join(&rel)) else { continue };
            if !there.contains(&rel) || self.store.read(&rel).ok().as_deref() != Some(here.as_slice()) {
                wrong.push(rel);
            }
        }
        Ok(wrong)
    }
}

/// Notes and other text: their lines can be joined.
fn is_text(rel: &str) -> bool {
    rel.ends_with(".md") || rel.ends_with(".env") || rel.ends_with(".txt")
}

/// A conflict copy's name not taken yet.
fn free_name(name: &str, there: &BTreeMap<String, String>) -> String {
    if !there.contains_key(name) {
        return name.to_string();
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    (2..).map(|i| format!("{stem}-{i}.{ext}")).find(|n| !there.contains_key(n)).unwrap_or_default()
}

/// The copies cloud programs make of a file two computers changed at once,
/// and the file each is of: new since the last sync, beside a file of the
/// same name. Told by their form, in any language: OneDrive adds
/// `-COMPUTERNAME`, Dropbox a date in parentheses, Google Drive `(1)`,
/// iCloud ` 2`.
fn client_copies(there: &BTreeMap<String, String>, base: &BTreeMap<String, Was>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for rel in there.keys() {
        if base.contains_key(rel) || rel.contains(".conflict.") {
            continue;
        }
        let (dir, name) = match rel.rsplit_once('/') {
            Some((d, n)) => (format!("{d}/"), n),
            None => (String::new(), rel.as_str()),
        };
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) => (s, format!(".{e}")),
            None => (name, String::new()),
        };
        let original = |s: &str| {
            let o = format!("{dir}{s}{ext}");
            (there.contains_key(&o) && o != *rel).then_some(o)
        };
        // Dropbox: "name (… 2026-01-02).md"; Google Drive: "name (1).md"
        if let Some(open) = stem.rfind(" (")
            && stem.ends_with(')')
        {
            let inside = &stem[open + 2..stem.len() - 1];
            let numbered = !inside.is_empty() && inside.chars().all(|c| c.is_ascii_digit());
            if (numbered || has_date(inside))
                && let Some(o) = original(&stem[..open])
            {
                out.push((rel.clone(), o));
                continue;
            }
        }
        // iCloud: "name 2.md"
        if let Some((s, n)) = stem.rsplit_once(' ')
            && !n.is_empty()
            && n.chars().all(|c| c.is_ascii_digit())
            && let Some(o) = original(s)
        {
            out.push((rel.clone(), o));
            continue;
        }
        // OneDrive: "name-COMPUTERNAME.md" (a computer's name: capitals, digits, -)
        if let Some(at) = stem.find('-') {
            for (i, _) in stem.match_indices('-').filter(|(i, _)| *i >= at) {
                let who = &stem[i + 1..];
                let computer = who.len() >= 2
                    && who.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
                    && who.chars().any(|c| c.is_ascii_uppercase());
                if computer && let Some(o) = original(&stem[..i]) {
                    out.push((rel.clone(), o));
                    break;
                }
            }
        }
    }
    out
}

/// "Ann's conflicted copy 2026-01-02" has a date in it.
fn has_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.windows(10).any(|w| {
        w[4] == b'-'
            && w[7] == b'-'
            && w[..4].iter().all(u8::is_ascii_digit)
            && w[5..7].iter().all(u8::is_ascii_digit)
            && w[8..].iter().all(u8::is_ascii_digit)
    })
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/sync/store.rs"]
mod tests;
