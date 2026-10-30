//! Writing notes safely when many `pg` processes run at once: one writer
//! per project at a time (the lock), and files replaced in one step (a
//! reader sees the old note or the new one, never half of it).

use anyhow::{Context, Result, bail};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

/// How long a writer waits for another one before giving up.
const LOCK_WAIT: Duration = Duration::from_secs(30);

/// The project's write lock, held until dropped.
pub struct WriteLock {
    _file: File,
}

impl WriteLock {
    pub fn take(meta_dir: &Path) -> Result<WriteLock> {
        std::fs::create_dir_all(meta_dir)?;
        let path = meta_dir.join("lock");
        let file = OpenOptions::new().create(true).truncate(false).write(true).open(&path)?;
        let start = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(WriteLock { _file: file }),
                Err(std::fs::TryLockError::WouldBlock) if start.elapsed() < LOCK_WAIT => {
                    std::thread::sleep(Duration::from_millis(15));
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    bail!("another pg kept the project busy for {}s", LOCK_WAIT.as_secs())
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(e).context("lock the project"),
            }
        }
    }
}

/// Replaces `path` with `data` in one step, creating missing folders.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().context("file has no folder")?;
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("note");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    {
        let mut f = File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    // Windows refuses the rename while another process reads the old file
    // for an instant (antivirus, indexers, a parallel pg): try again briefly
    let mut tries = 0;
    loop {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(_) if tries < 50 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e).with_context(|| format!("write {}", path.display()));
            }
        }
    }
}

/// Reads a text file, tolerating the brief moment a rename holds it on
/// Windows.
pub fn read_text(path: &Path) -> std::io::Result<String> {
    let mut tries = 0;
    loop {
        match std::fs::read(path) {
            Ok(b) => return Ok(String::from_utf8_lossy(&b).into_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && tries < 50 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_replace_whole_files() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a/b/note.md");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(read_text(&p).unwrap(), "two");
        let leftovers = std::fs::read_dir(p.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1);
    }

    #[test]
    fn the_lock_is_exclusive_until_dropped() {
        let d = tempfile::tempdir().unwrap();
        let first = WriteLock::take(d.path()).unwrap();
        let f = OpenOptions::new().write(true).open(d.path().join("lock")).unwrap();
        assert!(f.try_lock().is_err());
        drop(first);
        assert!(f.try_lock().is_ok());
    }
}
