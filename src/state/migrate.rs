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
const STEPS: &[fn(&Transaction) -> Result<()>] = &[v1_tables];

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
    let db = Connection::open_with_flags(state_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    refuse_newer(version(&db)?)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn open(path: &Path) -> Connection {
        crate::db::open(path).unwrap()
    }

    fn backups(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir.join("backup"))
            .map(|r| r.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn a_new_file_gets_the_tables_and_no_backup() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("state.db");
        let mut db = open(&path);
        run(&mut db, &path).unwrap();
        assert_eq!(version(&db).unwrap(), VERSION);
        assert!(has_column(&db, "shown", "full_at").unwrap());
        assert!(backups(d.path()).is_empty());
    }

    #[test]
    fn data_of_pg_0_2_0_is_kept_and_backed_up_once() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("state.db");
        {
            // what pg 0.2.0 made: the tables, the column added later, no version
            let db = open(&path);
            db.execute_batch(
                "CREATE TABLE events(at INTEGER, conversation TEXT, action TEXT, target TEXT, detail TEXT);
                 CREATE TABLE shown(conversation TEXT, ref TEXT, at INTEGER, PRIMARY KEY(conversation, ref));
                 ALTER TABLE shown ADD COLUMN full_at INTEGER NOT NULL DEFAULT 0;
                 INSERT INTO events VALUES (1, 'c1', 'finish', '', 'Handed off.');
                 INSERT INTO shown VALUES ('c1', 'r1', 5, 5);",
            )
            .unwrap();
        }
        let mut db = open(&path);
        run(&mut db, &path).unwrap();
        assert_eq!(version(&db).unwrap(), VERSION);
        let detail: String = db.query_row("SELECT detail FROM events", [], |r| r.get(0)).unwrap();
        assert_eq!(detail, "Handed off.");
        let full_at: i64 = db.query_row("SELECT full_at FROM shown", [], |r| r.get(0)).unwrap();
        assert_eq!(full_at, 5);

        // the copy holds the data as it was, version and all
        let names = backups(d.path());
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].starts_with("state-v0-") && names[0].ends_with(".db"), "{names:?}");
        let copy = Connection::open(d.path().join("backup").join(&names[0])).unwrap();
        assert_eq!(version(&copy).unwrap(), 0);
        let n: i64 = copy.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);

        // opening again changes nothing
        run(&mut db, &path).unwrap();
        assert_eq!(backups(d.path()).len(), 1);
    }

    #[test]
    fn newer_data_is_refused_and_left_alone() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("state.db");
        {
            let db = open(&path);
            db.execute_batch("CREATE TABLE future(x); INSERT INTO future VALUES (1);").unwrap();
            db.pragma_update(None, "user_version", VERSION + 1).unwrap();
        }
        let e = check(&path).unwrap_err().to_string();
        assert!(e.contains("opened by a newer Paragma") && e.contains("nothing was changed"), "{e}");
        let mut db = open(&path);
        assert!(run(&mut db, &path).is_err());
        assert_eq!(version(&db).unwrap(), VERSION + 1);
        let tables: i64 = db.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0)).unwrap();
        assert_eq!(tables, 1);
        assert!(backups(d.path()).is_empty());
        assert!(check(&d.path().join("missing.db")).is_ok());
    }

    #[test]
    fn only_the_newest_backups_are_kept() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("backup");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..5 {
            let part = dir.join(format!("state-v{i}-100{i}-77.part"));
            std::fs::write(&part, "x").unwrap();
            // distinct times, oldest first
            let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + i);
            std::fs::File::options().write(true).open(&part).unwrap().set_modified(t).unwrap();
            keep(&part).unwrap();
        }
        assert_eq!(backups(d.path()), ["state-v2-1002.db", "state-v3-1003.db", "state-v4-1004.db"]);
    }

    #[test]
    fn many_processes_move_the_data_once() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("state.db");
        open(&path)
            .execute_batch(
                "CREATE TABLE events(at INTEGER, conversation TEXT, action TEXT, target TEXT, detail TEXT);
                 INSERT INTO events VALUES (1, 'c1', 'finish', '', 'Handed off.');",
            )
            .unwrap();
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut db = crate::db::open(&path)?;
                    run(&mut db, &path)
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap().unwrap();
        }
        assert_eq!(version(&open(&path)).unwrap(), VERSION);
        let names = backups(d.path());
        assert_eq!(names.len(), 1, "{names:?}");
    }
}
