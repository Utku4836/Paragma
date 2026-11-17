//! Keeping `state.db` across versions of pg. It holds what cannot be
//! rebuilt, so a new version moves it forward in numbered steps and never
//! drops a table; before the first step it copies the file to
//! `.pg/backup/`. A pg older than the data does not touch it.
//!
//! The data's version is SQLite's `user_version`. Version 0 is a new file,
//! or one written by pg 0.2.0, which had no version yet.

use anyhow::{Result, bail};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use std::path::{Path, PathBuf};

/// The steps, in order: step `i` moves the data from version `i` to `i + 1`.
/// Only add steps at the end; each one adds tables or columns, or copies.
const STEPS: &[fn(&Transaction) -> Result<()>] = &[v1_tables, v2_outcomes, v3_shown_version, v4_run_symbols];

/// The version this pg writes.
pub const VERSION: i64 = STEPS.len() as i64;

/// How many backups a project keeps.
const BACKUPS: usize = 3;

/// The tables of pg 0.2.0, which they already are in its files.
fn v1_tables(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        "
CREATE TABLE IF NOT EXISTS sessions(
    conversation TEXT PRIMARY KEY,
    folder TEXT,
    started INTEGER,
    last INTEGER
);
CREATE TABLE IF NOT EXISTS events(
    at INTEGER,
    conversation TEXT,
    action TEXT,
    target TEXT,
    detail TEXT
);
CREATE INDEX IF NOT EXISTS events_conversation ON events(conversation, at);
CREATE TABLE IF NOT EXISTS shown(
    conversation TEXT,
    ref TEXT,
    at INTEGER,
    PRIMARY KEY(conversation, ref)
);
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS runs(
    at INTEGER,
    conversation TEXT,
    command TEXT,
    exit INTEGER,
    seconds REAL,
    failing TEXT,
    passed INTEGER,
    errors TEXT,
    changed TEXT
);
CREATE INDEX IF NOT EXISTS runs_command ON runs(command, at);
",
    )?;
    if !has_column(tx, "shown", "full_at")? {
        tx.execute_batch("ALTER TABLE shown ADD COLUMN full_at INTEGER NOT NULL DEFAULT 0")?;
    }
    Ok(())
}

/// Whether what memory showed helped: a known fix shown, then the test it
/// was for passing in the same conversation (1), or still failing at its
/// handoff (0).
fn v2_outcomes(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        "
CREATE TABLE IF NOT EXISTS outcomes(
    ref TEXT,
    at INTEGER,
    conversation TEXT,
    command TEXT,
    helped INTEGER
);
CREATE INDEX IF NOT EXISTS outcomes_ref ON outcomes(ref, at);
",
    )?;
    Ok(())
}

/// Which version of a note or rule a conversation was shown in full: a
/// note changed since then is shown in full again.
fn v3_shown_version(tx: &Transaction) -> Result<()> {
    if !has_column(tx, "shown", "version")? {
        tx.execute_batch("ALTER TABLE shown ADD COLUMN version INTEGER NOT NULL DEFAULT 0")?;
    }
    Ok(())
}

/// The functions changed (by git) when a run ran, each with the hash of
/// its body then: what changed after a failing run is what fixed it.
fn v4_run_symbols(tx: &Transaction) -> Result<()> {
    if !has_column(tx, "runs", "symbols")? {
        tx.execute_batch("ALTER TABLE runs ADD COLUMN symbols TEXT NOT NULL DEFAULT ''")?;
    }
    Ok(())
}

fn has_column(db: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut st = db.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = st.query_map([], |r| r.get::<_, String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names.iter().any(|n| n == column))
}

fn version(db: &Connection) -> Result<i64> {
    Ok(db.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// Brings the open `state.db` at `path` up to this pg's version.
pub fn run(db: &mut Connection, path: &Path) -> Result<()> {
    let from = version(db)?;
    if from == VERSION {
        return Ok(());
    }
    refuse_newer(from)?;
    let has_data: bool = db.query_row("SELECT COUNT(*) > 0 FROM sqlite_master", [], |r| r.get(0))?;
    // the copy is taken before the write lock (SQLite cannot copy inside a
    // transaction); a process that finds the work done drops its copy
    let copy = if has_data { Some(backup(db, path, from)?) } else { None };
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let now = version(&tx)?;
    if now >= VERSION {
        drop(tx);
        if let Some(c) = copy {
            let _ = std::fs::remove_file(c);
        }
        return refuse_newer(now);
    }
    for step in &STEPS[now as usize..] {
        step(&tx)?;
    }
    tx.pragma_update(None, "user_version", VERSION)?;
    tx.commit()?;
    if let Some(c) = copy {
        keep(&c)?;
    }
    Ok(())
}

/// Stops before a pg older than the data changes anything in the project.
/// Cheap: reads the version only when the file exists.
pub fn check(state_path: &Path) -> Result<()> {
    if !state_path.is_file() {
        return Ok(());
    }
    let found = Connection::open_with_flags(state_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(anyhow::Error::from)
        .and_then(|db| version(&db));
    match found {
        Ok(v) => refuse_newer(v),
        // a damaged file says nothing of who wrote it: State::open renews it
        Err(e) if crate::db::broken(&e) => Ok(()),
        Err(e) => Err(e),
    }
}

fn refuse_newer(found: i64) -> Result<()> {
    if found > VERSION {
        bail!(
            "this project was opened by a newer Paragma (its data is version {found}, this pg {} knows {VERSION}); \
             nothing was changed. Update: npm install -g paragma@latest",
            env!("CARGO_PKG_VERSION")
        );
    }
    Ok(())
}

/// Copies the data as it is, to `.pg/backup/state-v<version>-<time>.part`.
fn backup(db: &Connection, path: &Path, from: i64) -> Result<PathBuf> {
    let dir = path.with_file_name("backup");
    std::fs::create_dir_all(&dir)?;
    // unique to this call: others may be copying at the same moment
    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let part = dir.join(format!("state-v{from}-{}-{}.{call}.part", crate::output::now(), std::process::id()));
    db.execute("VACUUM INTO ?1", [part.to_string_lossy()])?;
    Ok(part)
}

/// Keeps a finished copy under its name and drops the oldest past `BACKUPS`.
fn keep(part: &Path) -> Result<()> {
    let name = part.file_name().unwrap_or_default().to_string_lossy();
    // state-v0-<time>-<pid>.<call>.part -> state-v0-<time>.db
    let stem = name.trim_end_matches(".part");
    let stem = stem.rsplit_once('-').map(|(s, _)| s).unwrap_or(stem);
    let dir = part.parent().unwrap_or(Path::new("."));
    std::fs::rename(part, dir.join(format!("{stem}.db")))?;
    let mut all: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().unwrap_or_default().to_string_lossy();
            n.starts_with("state-") && n.ends_with(".db")
        })
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .collect();
    all.sort();
    let extra = all.len().saturating_sub(BACKUPS);
    for (_, old) in all.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
    Ok(())
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/state/migrate.rs"]
mod tests;
