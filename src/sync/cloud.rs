//! The cloud folders on this computer: where OneDrive, Google Drive,
//! Dropbox and iCloud keep their files here. A memory in one is kept by the
//! cloud and its path below the cloud's own folder, so another computer,
//! whose cloud folder is somewhere else, finds it in its own; and by the
//! memory's id, when that path is not there (a folder named in another
//! language, moved by hand).

use super::MARKER;
use std::path::{Path, PathBuf};

/// A cloud with a folder on this computer.
#[derive(Clone, Debug, PartialEq)]
pub struct Cloud {
    pub id: &'static str,
    pub name: &'static str,
    pub root: PathBuf,
}

pub const CLOUDS: [(&str, &str); 4] =
    [("onedrive", "OneDrive"), ("gdrive", "Google Drive"), ("dropbox", "Dropbox"), ("icloud", "iCloud Drive")];

pub fn name(id: &str) -> &'static str {
    CLOUDS.iter().find(|c| c.0 == id).map(|c| c.1).unwrap_or("a folder")
}

fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_default()
}

/// `PARAGMA_CLOUDS="onedrive=C:\x;gdrive=D:\y"` names the clouds' folders
/// instead of finding them (tests, an unusual setup).
fn named() -> Option<Vec<(String, PathBuf)>> {
    let v = std::env::var("PARAGMA_CLOUDS").ok()?;
    Some(
        v.split(';')
            .filter_map(|p| p.split_once('='))
            .map(|(k, p)| (k.trim().to_string(), PathBuf::from(p.trim())))
            .collect(),
    )
}

/// The clouds that have a folder here.
pub fn found() -> Vec<Cloud> {
    CLOUDS.iter().filter_map(|(id, name)| root(id).map(|root| Cloud { id, name, root })).collect()
}

/// The folder a cloud keeps its files in here.
pub fn root(id: &str) -> Option<PathBuf> {
    if let Some(n) = named() {
        return n.into_iter().find(|(k, _)| k == id).map(|(_, p)| p).filter(|p| p.is_dir());
    }
    let h = home();
    let dir = |p: PathBuf| p.is_dir().then_some(p);
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from).and_then(dir);
    // macOS keeps every cloud's folder in ~/Library/CloudStorage/<Name>-<account>
    let storage = |prefix: &str| -> Option<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(h.join("Library").join("CloudStorage"))
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(prefix)))
            .collect();
        found.sort();
        found.into_iter().next()
    };
    match id {
        "onedrive" => env("OneDrive")
            .or_else(|| env("OneDriveConsumer"))
            .or_else(|| env("OneDriveCommercial"))
            .or_else(|| storage("OneDrive"))
            .or_else(|| dir(h.join("OneDrive"))),
        "gdrive" => google_drive().or_else(|| storage("GoogleDrive")).or_else(|| dir(h.join("Google Drive"))),
        "dropbox" => dropbox().or_else(|| storage("Dropbox")).or_else(|| dir(h.join("Dropbox"))),
        "icloud" => dir(h.join("iCloudDrive"))
            .or_else(|| dir(h.join("Library").join("Mobile Documents").join("com~apple~CloudDocs"))),
        _ => None,
    }
}

/// Google Drive for desktop shows the drive as a disk of its own: the one
/// with its shortcuts folder at the top.
fn google_drive() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    (b'D'..=b'Z')
        .map(|l| PathBuf::from(format!("{}:\\", l as char)))
        .find(|r| r.join(".shortcut-targets-by-id").is_dir())
}

/// Dropbox says where its folder is in its info.json.
fn dropbox() -> Option<PathBuf> {
    let mut places = vec![home().join(".dropbox").join("info.json")];
    for k in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(p) = std::env::var_os(k) {
            places.push(PathBuf::from(p).join("Dropbox").join("info.json"));
        }
    }
    for p in places {
        let Ok(t) = std::fs::read_to_string(&p) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else { continue };
        for kind in ["personal", "business"] {
            if let Some(path) = v[kind]["path"].as_str()
                && Path::new(path).is_dir()
            {
                return Some(PathBuf::from(path));
            }
        }
    }
    None
}

/// The cloud a folder is in, and its path below the cloud's folder with
/// '/'; ("", the whole path) for a folder of no cloud.
pub fn portable(path: &Path) -> (String, String) {
    for c in found() {
        if let Ok(rest) = path.strip_prefix(&c.root) {
            return (c.id.to_string(), rest.to_string_lossy().replace('\\', "/"));
        }
    }
    (String::new(), path.to_string_lossy().into_owned())
}

/// The folder of a memory on this computer: the cloud's folder here and
/// the path below it; when that holds no memory of this id, the memory
/// looked for by its id in that cloud.
pub fn resolve(cloud: &str, rel: &str, id: &str) -> Option<PathBuf> {
    if cloud.is_empty() {
        let p = PathBuf::from(rel);
        return p.is_dir().then_some(p);
    }
    let root = root(cloud)?;
    let p = root.join(rel);
    if marker_id(&p).is_some_and(|m| id.is_empty() || m == id) {
        return Some(p);
    }
    if !id.is_empty()
        && let Some(found) = search(&root, id)
    {
        return Some(found);
    }
    // a new memory there: its folder made on connecting
    (marker_id(&p).is_none()).then_some(p)
}

/// A memory's id, from the marker in a folder.
pub fn marker_id(dir: &Path) -> Option<String> {
    let t = std::fs::read_to_string(dir.join(MARKER)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&t).ok()?;
    Some(v["id"].as_str().unwrap_or("").to_string())
}

/// The folder holding the memory with this id, below `root`: a few
/// levels down, a bounded number of folders looked at.
pub fn search(root: &Path, id: &str) -> Option<PathBuf> {
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0)]);
    let mut looked = 0;
    while let Some((dir, depth)) = queue.pop_front() {
        looked += 1;
        if looked > 3000 {
            return None;
        }
        // no id: any memory
        if marker_id(&dir).is_some_and(|m| id.is_empty() || m == id) {
            return Some(dir);
        }
        if depth >= 3 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name.starts_with('$') {
                continue;
            }
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                queue.push_back((e.path(), depth + 1));
            }
        }
    }
    None
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/sync/cloud.rs"]
mod tests;
