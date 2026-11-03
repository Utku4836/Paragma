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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn many_open_a_new_file_at_once() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("new.db");
        let threads: Vec<_> = (0..16)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let db = open(&path)?;
                    db.execute_batch("CREATE TABLE IF NOT EXISTS t(x); INSERT INTO t VALUES (1);")?;
                    anyhow::Ok(())
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap().unwrap();
        }
        let db = open(&path).unwrap();
        let mode: String = db.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
        let rows: i64 = db.query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 16);
    }
}
