//! Which notes exist and how they look on disk, from directory listings
//! only (on Windows the listing carries the times and sizes, so no file is
//! opened): about 15 ms for 1,500 notes.

use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileState {
    /// Path in the project, with '/'.
    pub rel: String,
    /// Modification time, nanoseconds since the epoch.
    pub mtime: i64,
    pub size: u64,
}

/// Every `.md` note of a project; dot folders (`.pg`, `.archive`, `.obsidian`)
/// are left out.
pub fn notes(dir: &Path) -> Vec<FileState> {
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<FileState>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        if meta.is_dir() {
            walk(&e.path(), &rel, out);
        } else if name.ends_with(".md") {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as i64)
                .unwrap_or(0);
            out.push(FileState { rel, mtime, size: meta.len() });
        }
    }
}

/// Folders of a project (for `pg ls`), dot folders left out.
/// Every note of every project below the projects folder that keeps files
/// (an "## Attachments" section): its path under that folder, and text.
pub fn all_notes(projects: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(projects) else { return out };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !e.path().is_dir() {
            continue;
        }
        for n in notes(&e.path()) {
            let Ok(text) = std::fs::read_to_string(e.path().join(&n.rel)) else { continue };
            if text.contains(
                "
## Attachments",
            ) {
                out.push((format!("{name}/{}", n.rel), text));
            }
        }
    }
    out
}

pub fn folders(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn go(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            out.push(rel.clone());
            go(&e.path(), &rel, out);
        }
    }
    go(dir, "", &mut out);
    out.sort();
    out
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/project/scan.rs"]
mod tests;
