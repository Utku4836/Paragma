//! Opening the project's SQLite files (`.pg/*.db`), the same way for all.

use anyhow::Result;
use rusqlite::{Connection, ErrorCode};
use std::path::Path;
use std::time::{Duration, Instant};

/// How long a call waits for another process holding the file.
const WAIT: Duration = Duration::from_secs(60);

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let db = Connection::open(path)?;
    db.busy_timeout(WAIT)?;
    write_ahead_log(&db)?;
    db.execute_batch("PRAGMA synchronous = NORMAL;")?;
    Ok(db)
}

/// Readers and a writer at the same time. The mode is kept in the file, but
/// turning it on for a new file needs the file alone, and SQLite answers
/// "locked" at once instead of waiting when other processes have it open
/// (many conversations opening a new project together), so this waits.
fn write_ahead_log(db: &Connection) -> Result<()> {
    let start = Instant::now();
    loop {
        match db.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0)) {
            Ok(_) => return Ok(()),
            Err(e) if busy(&e) && start.elapsed() < WAIT => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.into()),
        }
    }
}

pub fn busy(e: &rusqlite::Error) -> bool {
    matches!(e.sqlite_error_code(), Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked))
}

/// Whether an error says a database file is damaged (not a database, or
/// broken inside), as opposed to busy or missing.
pub fn broken(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<rusqlite::Error>().is_some_and(|e| {
            matches!(e.sqlite_error_code(), Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt))
        })
    })
}

/// Opens one of pg's own databases that can be made again (the indexes,
/// from the notes and the code) or started anew (`state.db`), and gets it
/// ready with `prepare` (its tables, brought up to date). When the file
/// turns out damaged (a crash of the computer, a disk, a sync tool), it is
/// moved aside as `<name>.broken-<time>` for a look and a new one is made,
/// instead of every command failing on it.
pub fn open_or_renew<T>(path: &Path, mut prepare: impl FnMut(Connection) -> Result<T>) -> Result<T> {
    match open(path).and_then(&mut prepare) {
        Err(e) if broken(&e) => {
            set_aside(path)?;
            open(path).and_then(prepare)
        }
        other => other,
    }
}

/// Moves a damaged database and its journal aside. Another pg may have
/// done it already and made a new one meanwhile: the file is moved only
/// while it is still damaged.
pub fn set_aside(path: &Path) -> Result<()> {
    let start = Instant::now();
    let stamp = crate::output::now();
    loop {
        if !path.exists() || !still_broken(path) {
            return Ok(());
        }
        let aside = |suffix: &str| {
            let mut name = path.file_name().unwrap_or_default().to_os_string();
            name.push(format!("{suffix}.broken-{stamp}"));
            path.with_file_name(name)
        };
        // the journal belongs to the damaged file: it goes with it
        let mut moved = std::fs::rename(path, aside("")).is_ok();
        for side in ["-wal", "-shm"] {
            let mut p = path.as_os_str().to_os_string();
            p.push(side);
            let p = std::path::PathBuf::from(p);
            if p.exists() {
                moved &= std::fs::rename(&p, aside(side)).is_ok() || !p.exists();
            }
        }
        if moved {
            return Ok(());
        }
        // Windows refuses while another pg still has it open: it lets go
        // as soon as it meets the same damage
        if start.elapsed() > WAIT {
            anyhow::bail!("{} is damaged and another program keeps it open", path.display());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Whether pg's database at `path` is damaged (a missing one is not).
pub fn damaged(path: &Path) -> bool {
    path.is_file() && still_broken(path)
}

/// Whether the file at `path` is damaged now (opened afresh): a busy file
/// is not.
fn still_broken(path: &Path) -> bool {
    let check = || -> Result<String> {
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(WAIT)?;
        Ok(db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?)
    };
    match check() {
        Ok(answer) => answer != "ok",
        Err(e) => broken(&e),
    }
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../unit/src/db.rs"]
mod tests;
