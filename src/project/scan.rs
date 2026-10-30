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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_notes_but_not_dot_folders() {
        let d = tempfile::tempdir().unwrap();
        for f in ["a.md", "x/b.md", "x/y/c.md", ".pg/index.md", "x/readme.txt"] {
            let p = d.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "text").unwrap();
        }
        let rels: Vec<String> = notes(d.path()).into_iter().map(|f| f.rel).collect();
        assert_eq!(rels, vec!["a.md", "x/b.md", "x/y/c.md"]);
        assert_eq!(folders(d.path()), vec!["x", "x/y"]);
    }
}
