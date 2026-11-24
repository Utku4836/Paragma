//! The file calls of sync, each error naming the file or folder it was
//! about (the system's own error says neither).

use anyhow::{Context, Result};
use std::path::Path;

pub fn read(p: impl AsRef<Path>) -> Result<Vec<u8>> {
    let p = p.as_ref();
    std::fs::read(p).with_context(|| format!("read {}", p.display()))
}

pub fn write(p: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> Result<()> {
    let p = p.as_ref();
    std::fs::write(p, bytes).with_context(|| format!("write {}", p.display()))
}

pub fn create_dir_all(p: impl AsRef<Path>) -> Result<()> {
    let p = p.as_ref();
    std::fs::create_dir_all(p).with_context(|| format!("make the folder {}", p.display()))
}

pub fn remove_file(p: impl AsRef<Path>) -> Result<()> {
    let p = p.as_ref();
    std::fs::remove_file(p).with_context(|| format!("remove {}", p.display()))
}

pub fn metadata(p: impl AsRef<Path>) -> Result<std::fs::Metadata> {
    let p = p.as_ref();
    std::fs::metadata(p).with_context(|| format!("look at {}", p.display()))
}

pub fn read_dir(p: impl AsRef<Path>) -> Result<std::fs::ReadDir> {
    let p = p.as_ref();
    std::fs::read_dir(p).with_context(|| format!("list the folder {}", p.display()))
}

/// Removes a file; one already gone (taken out meanwhile) is no error.
pub fn remove_gone(p: impl AsRef<Path>) -> Result<()> {
    let p = p.as_ref();
    match std::fs::remove_file(p) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).with_context(|| format!("remove {}", p.display())),
        _ => Ok(()),
    }
}
