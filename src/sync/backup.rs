//! The whole memory in one zip, and back: `pg sync backup <folder>` writes
//! `paragma-backup-2026-11-23-1015.zip` with every project's notes, files
//! kept with notes and archive (not the indexes: they are made again), and
//! a manifest of every file with its size and hash, so a damaged or partial
//! zip is known before anything is written. `pg sync restore <zip>` checks
//! it, copies the memory as it is first, then joins the backup with it
//! (merge) or puts it in its place (replace), and checks what it wrote.
//!
//! A daily backup, when turned on: pg's first command of a day writes one
//! to the chosen folder in the background and keeps the last few.

use super::repo::{kept, remove_empty_dirs, walk};
use crate::project::Home;
use crate::project::attach::hash;
use crate::project::files::write_atomic;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The start of every backup's file name.
pub const PREFIX: &str = "paragma-backup-";
/// The start of a daily backup's name: only these are pruned.
pub const DAILY: &str = "paragma-backup-daily-";

/// What a backup holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub version: String,
    pub created: i64,
    pub host: String,
    pub id: String,
    /// The .env files are in it.
    pub env: bool,
    /// Path under projects/, size, hash.
    pub files: Vec<(String, u64, String)>,
}

impl Manifest {
    pub fn projects(&self) -> usize {
        let mut p: Vec<&str> = self.files.iter().map(|f| f.0.split('/').next().unwrap_or("")).collect();
        p.sort();
        p.dedup();
        p.len()
    }
}

/// A backup's file name for a moment.
pub fn name(prefix: &str, at: i64) -> String {
    let d = crate::output::date(at);
    let s = at.rem_euclid(86_400);
    let t = format!("{:02}{:02}", s / 3600, s % 3600 / 60);
    if prefix == DAILY { format!("{prefix}{d}.zip") } else { format!("{prefix}{d}-{t}.zip") }
}

/// The memory's files that a backup holds, by their path under projects/.
fn files(home: &Home, env: bool) -> Result<Vec<String>> {
    let mut out = Vec::new();
    walk(&home.projects_dir(), "", &mut out, &|rel, is_dir| kept(rel, is_dir, env))?;
    out.sort();
    Ok(out)
}

/// Writes the whole memory into `path` (a .zip), whole or not at all.
pub fn write(home: &Home, path: &Path, env: bool) -> Result<Manifest> {
    let projects = home.projects_dir();
    let mut list = Vec::new();
    let dir = path.parent().context("a folder for the backup")?;
    std::fs::create_dir_all(dir).with_context(|| format!("make {}", dir.display()))?;
    let tmp = dir.join(format!(".~pg-{}.zip", std::process::id()));
    let result = (|| -> Result<Manifest> {
        let f = std::fs::File::create(&tmp).with_context(|| format!("write {}", tmp.display()))?;
        let mut z = zip::ZipWriter::new(std::io::BufWriter::new(f));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for rel in files(home, env)? {
            let Ok(bytes) = std::fs::read(projects.join(&rel)) else { continue };
            z.start_file(format!("projects/{rel}"), opts.large_file(bytes.len() as u64 >= u32::MAX as u64))?;
            z.write_all(&bytes)?;
            list.push((rel, bytes.len() as u64, hash(&bytes)));
        }
        let m = Manifest {
            version: env!("CARGO_PKG_VERSION").to_string(),
            created: crate::output::now(),
            host: super::repo::host(),
            id: super::Config::load(home).map(|c| c.id).unwrap_or_default(),
            env,
            files: list,
        };
        z.start_file("manifest.json", opts)?;
        z.write_all(to_json(&m).to_string().as_bytes())?;
        z.finish()?.flush()?;
        Ok(m)
    })();
    match result {
        Ok(m) => {
            std::fs::rename(&tmp, path).with_context(|| format!("put {} in place", path.display()))?;
            Ok(m)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn to_json(m: &Manifest) -> Value {
    json!({
        "paragma_backup": 1, "version": m.version, "created": m.created, "host": m.host, "id": m.id, "env": m.env,
        "files": m.files.iter().map(|(p, s, h)| json!({ "path": p, "size": s, "hash": h })).collect::<Vec<_>>(),
    })
}

fn open(path: &Path) -> Result<zip::ZipArchive<std::fs::File>> {
    let f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    zip::ZipArchive::new(f).with_context(|| format!("{} is not a zip", path.display()))
}

/// What a backup says it holds (read from its manifest only).
pub fn manifest(path: &Path) -> Result<Manifest> {
    let mut z = open(path)?;
    let mut text = String::new();
    z.by_name("manifest.json")
        .with_context(|| format!("{} is not a Paragma backup (no manifest)", path.display()))?
        .read_to_string(&mut text)
        .with_context(|| format!("{} is damaged", path.display()))?;
    let v: Value = serde_json::from_str(&text).context("the backup's manifest")?;
    if v["paragma_backup"].as_i64().is_none() {
        bail!("{} is not a Paragma backup", path.display());
    }
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    Ok(Manifest {
        version: s("version"),
        created: v["created"].as_i64().unwrap_or(0),
        host: s("host"),
        id: s("id"),
        env: v["env"].as_bool().unwrap_or(false),
        files: v["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| {
                Some((f["path"].as_str()?.to_string(), f["size"].as_u64()?, f["hash"].as_str()?.to_string()))
            })
            .collect(),
    })
}

/// A path a backup may write: below projects/, no way out of it.
fn safe(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel.contains(':')
        && rel.split('/').all(|p| !p.is_empty() && p != "." && p != ".." && p != ".pg" && p != ".git")
}

/// A backup's files: their path under projects/, their bytes.
pub type Files = Vec<(String, Vec<u8>)>;

/// Every file of a backup read and checked against its manifest: the
/// files, by path. Nothing is written.
pub fn read_checked(path: &Path) -> Result<(Manifest, Files)> {
    let m = manifest(path)?;
    let mut z = open(path)?;
    let mut out = Vec::with_capacity(m.files.len());
    for (rel, size, want) in &m.files {
        if !safe(rel) {
            bail!("the backup names a file outside the memory ({rel}): not used");
        }
        let mut f =
            z.by_name(&format!("projects/{rel}")).with_context(|| format!("{rel} is missing from the backup"))?;
        if f.enclosed_name().is_none() {
            bail!("the backup names a file outside the memory ({rel}): not used");
        }
        let mut bytes = Vec::with_capacity(*size as usize);
        f.read_to_end(&mut bytes).with_context(|| format!("{rel} in the backup is damaged: nothing was restored"))?;
        if bytes.len() as u64 != *size || hash(&bytes) != *want {
            bail!("{rel} in the backup is damaged (it is not as the manifest says): nothing was restored");
        }
        out.push((rel.clone(), bytes));
    }
    Ok((m, out))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// The backup's notes join the memory: new ones added, a note that
    /// differs kept beside as name.conflict.md.
    Merge,
    /// The memory becomes the backup.
    Replace,
}

/// What a restore did.
#[derive(Debug, Default)]
pub struct Restored {
    pub added: usize,
    pub same: usize,
    pub conflicts: Vec<String>,
    pub removed: usize,
}

/// Restores a backup. The memory is copied first (into pg's folder); the
/// copy is taken back in when anything fails, and deleted when all went
/// well.
pub fn restore(home: &Home, path: &Path, mode: Mode) -> Result<Restored> {
    let (m, entries) = read_checked(path)?;
    let _lock = super::SyncLock::take(home, std::time::Duration::from_secs(120))?;
    let safety = super::dir(home).join(name("before-restore-", crate::output::now()));
    write(home, &safety, true).context("copy the memory before restoring")?;
    let result = apply(home, &entries, mode, m.env).and_then(|r| {
        check(home, &entries, mode)?;
        Ok(r)
    });
    match result {
        Ok(r) => {
            let _ = std::fs::remove_file(&safety);
            drop(_lock);
            super::trigger(home);
            Ok(r)
        }
        Err(e) => {
            // as it was before
            let back = read_checked(&safety).and_then(|(_, b)| apply(home, &b, Mode::Replace, true));
            match back {
                Ok(_) => {
                    let _ = std::fs::remove_file(&safety);
                    Err(e.context("the restore failed; the memory is as it was before"))
                }
                Err(_) => {
                    Err(e.context(format!("the restore failed; the memory as it was is in {}", safety.display())))
                }
            }
        }
    }
}

fn apply(home: &Home, entries: &[(String, Vec<u8>)], mode: Mode, env: bool) -> Result<Restored> {
    let projects = home.projects_dir();
    let mut r = Restored::default();
    let names: std::collections::HashSet<&str> = entries.iter().map(|(p, _)| p.as_str()).collect();
    for (rel, bytes) in entries {
        let to = projects.join(rel);
        match std::fs::read(&to) {
            Ok(here) if here == *bytes => r.same += 1,
            Ok(_) if mode == Mode::Merge => {
                let side = free(&projects, &super::repo::conflict_name(rel));
                write_atomic(&projects.join(&side), bytes)?;
                r.conflicts.push(side);
            }
            _ => {
                write_atomic(&to, bytes)?;
                r.added += 1;
            }
        }
    }
    if mode == Mode::Replace {
        // the files the backup does not have go (the .env files only when
        // the backup holds them)
        for rel in files(home, env)? {
            if !names.contains(rel.as_str()) {
                std::fs::remove_file(projects.join(&rel))?;
                remove_empty_dirs(&projects, &rel);
                r.removed += 1;
            }
        }
    }
    Ok(r)
}

/// Every file of the backup is in the memory now (as itself, or beside a
/// note that differs, when merged).
fn check(home: &Home, entries: &[(String, Vec<u8>)], mode: Mode) -> Result<()> {
    let projects = home.projects_dir();
    for (rel, bytes) in entries {
        let here = std::fs::read(projects.join(rel)).ok();
        if here.as_deref() == Some(bytes.as_slice()) {
            continue;
        }
        if mode == Mode::Merge && here.is_some() {
            continue; // kept beside it
        }
        bail!("{rel} is not as the backup has it after restoring");
    }
    Ok(())
}

/// A name not taken in the folder.
fn free(projects: &Path, name: &str) -> String {
    if !projects.join(name).exists() {
        return name.to_string();
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    (2..).map(|i| format!("{stem}-{i}.{ext}")).find(|n| !projects.join(n).exists()).unwrap_or_default()
}

/// The backups on this computer: in Downloads, Desktop, Documents, the
/// daily backups' folder and the cloud folders (a level or two down),
/// newest first.
pub fn found(home: &Home) -> Vec<(PathBuf, Manifest)> {
    let h = std::env::home_dir().unwrap_or_default();
    let mut places: Vec<PathBuf> = ["Downloads", "Desktop", "Documents"].iter().map(|d| h.join(d)).collect();
    if let Some(d) = Daily::load(home).folder() {
        places.push(d);
    }
    places.extend(super::cloud::found().into_iter().map(|c| c.root));
    let mut out: Vec<(PathBuf, Manifest)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    fn look(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(t) = e.file_type() else { continue };
            if t.is_file() && name.starts_with(PREFIX) && name.ends_with(".zip") {
                out.push(e.path());
            } else if t.is_dir() && depth > 0 && !name.starts_with('.') && !name.starts_with('$') {
                look(&e.path(), depth - 1, out);
            }
        }
    }
    for p in places {
        let mut zips = Vec::new();
        look(&p, 2, &mut zips);
        for z in zips {
            let key = std::fs::canonicalize(&z).unwrap_or(z.clone());
            if seen.insert(key)
                && let Ok(m) = manifest(&z)
            {
                out.push((z, m));
            }
        }
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.1.created));
    out
}

// ---------- the daily backup ----------

/// The daily backup's settings, kept in `<home>/backup.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Daily {
    pub on: bool,
    pub folder: String,
    /// How many are kept.
    pub keep: usize,
    pub env: bool,
    /// When the last one was written.
    pub last: i64,
}

pub const KEEP: usize = 7;

impl Daily {
    fn path(home: &Home) -> PathBuf {
        home.root.join("backup.json")
    }

    pub fn load(home: &Home) -> Daily {
        let Some(v) =
            std::fs::read_to_string(Daily::path(home)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            return Daily { keep: KEEP, ..Daily::default() };
        };
        Daily {
            on: v["on"].as_bool().unwrap_or(false),
            folder: v["folder"].as_str().unwrap_or("").to_string(),
            keep: v["keep"].as_u64().map(|k| k.clamp(1, 365) as usize).unwrap_or(KEEP),
            env: v["env"].as_bool().unwrap_or(false),
            last: v["last"].as_i64().unwrap_or(0),
        }
    }

    pub fn save(&self, home: &Home) -> Result<()> {
        let v = json!({ "on": self.on, "folder": self.folder, "keep": self.keep, "env": self.env, "last": self.last });
        write_atomic(&Daily::path(home), serde_json::to_string_pretty(&v)?.as_bytes())
    }

    pub fn folder(&self) -> Option<PathBuf> {
        (!self.folder.is_empty()).then(|| PathBuf::from(&self.folder))
    }

    /// Turned on, and none written today yet.
    pub fn due(&self) -> bool {
        self.on
            && !self.folder.is_empty()
            && crate::output::date(self.last) != crate::output::date(crate::output::now())
    }
}

/// Starts today's backup in the background when it is due: quick, never
/// fails the command that called it.
pub fn daily_check(home: &Home) {
    // the backup's own pg does not start another
    if std::env::var_os("PARAGMA_DAILY").is_some() || !Daily::load(home).due() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let mut c = std::process::Command::new(exe);
    c.args(["sync", "backup", "--daily-now"])
        .env("PARAGMA_HOME", &home.root)
        .env("PARAGMA_DAILY", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let _ = super::detach(&mut c);
}

/// Today's backup, once (other pg started meanwhile find it done), and the
/// oldest daily ones beyond `keep` removed.
pub fn daily_now(home: &Home) -> Result<Option<PathBuf>> {
    std::fs::create_dir_all(&home.root)?;
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(home.root.join("backup.lock"))?;
    if f.try_lock().is_err() {
        return Ok(None);
    }
    let mut d = Daily::load(home);
    if !d.due() {
        return Ok(None);
    }
    let folder = d.folder().context("no folder for the daily backup")?;
    let now = crate::output::now();
    let path = folder.join(name(DAILY, now));
    write(home, &path, d.env)?;
    d.last = now;
    d.save(home)?;
    let mut old: Vec<PathBuf> = std::fs::read_dir(&folder)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.starts_with(DAILY) && n.ends_with(".zip")
            })
        })
        .collect();
    old.sort();
    while old.len() > d.keep {
        let _ = std::fs::remove_file(old.remove(0));
    }
    Ok(Some(path))
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/sync/backup.rs"]
mod tests;
