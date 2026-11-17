//! Which projects a code folder belongs to (`~/.paragma/projects/.links`).
//!
//! `pg open` links the folder it runs in; later commands in that folder or
//! below it know their project without being told. A project can be linked
//! to many folders and a folder to many projects (the most recently used
//! one is current).
//!
//! ```json
//! { "c:/dev/shop-api": [ { "project": "shop", "used": 1792000000 } ] }
//! ```

use crate::output::now;
use crate::project::{Home, files};
use anyhow::Result;
use serde_json::{Value, json};
use std::path::Path;

pub struct Links {
    /// folder -> [(project, last used, seconds since the epoch)]
    folders: Vec<(String, Vec<(String, i64)>)>,
}

impl Links {
    /// Links `folder` to `project` (used now), then saves. Callers changing
    /// links run under `Links::lock`.
    pub fn link(home: &Home, folder: &Path, project: &str) -> Result<()> {
        Links::change(home, |links| {
            // a folder above already linked to the project: just mark it used
            let above = ancestors(folder).find_map(|key| {
                links.folders.iter().position(|(f, list)| *f == key && list.iter().any(|(p, _)| p == project))
            });
            let i = match above.or_else(|| links.folders.iter().position(|(f, _)| *f == folder_key(folder))) {
                Some(i) => i,
                None => {
                    links.folders.push((folder_key(folder), Vec::new()));
                    links.folders.len() - 1
                }
            };
            let list = &mut links.folders[i].1;
            list.retain(|(p, _)| p != project);
            list.push((project.to_string(), now()));
        })
    }

    /// Takes the links of a project out: from these folders (keys as
    /// `folders_of` gives them), or from every folder. The folders unlinked.
    pub fn unlink(home: &Home, project: &str, only: Option<&[String]>) -> Result<Vec<String>> {
        let mut gone = Vec::new();
        Links::change(home, |links| {
            for (f, list) in &mut links.folders {
                if only.is_none_or(|o| o.contains(f)) && list.iter().any(|(p, _)| p == project) {
                    list.retain(|(p, _)| p != project);
                    gone.push(f.clone());
                }
            }
            links.folders.retain(|(_, list)| !list.is_empty());
        })?;
        Ok(gone)
    }

    /// A project got a new name.
    pub fn rename_project(home: &Home, old: &str, new: &str) -> Result<()> {
        Links::change(home, |links| {
            for (_, list) in &mut links.folders {
                for (p, _) in list.iter_mut() {
                    if p == old {
                        *p = new.to_string();
                    }
                }
            }
        })
    }

    /// A project is gone: its folders forget it.
    pub fn forget_project(home: &Home, name: &str) -> Result<()> {
        Links::change(home, |links| {
            for (_, list) in &mut links.folders {
                list.retain(|(p, _)| p != name);
            }
            links.folders.retain(|(_, list)| !list.is_empty());
        })
    }

    /// The folders linked to a project.
    pub fn folders_of(&self, project: &str) -> Vec<String> {
        self.folders.iter().filter(|(_, list)| list.iter().any(|(p, _)| p == project)).map(|(f, _)| f.clone()).collect()
    }

    /// Load, change and save under the links lock, so parallel `pg open`s
    /// do not drop each other's links.
    fn change(home: &Home, f: impl FnOnce(&mut Links)) -> Result<()> {
        let dir = home.projects_dir();
        let _lock = files::WriteLock::take(&dir.join(".pg"))?;
        // a file pg cannot read (edited by hand, a disk error) is not
        // written over with only the new link: it is kept aside
        let path = dir.join(".links");
        if let Ok(text) = std::fs::read_to_string(&path)
            && !matches!(serde_json::from_str::<Value>(&text), Ok(Value::Object(_)))
        {
            let aside = dir.join(format!(".links.broken-{}", crate::output::now()));
            std::fs::rename(&path, &aside)?;
            eprintln!(
                "pg: the folder links file could not be read; kept as {} (each project's folder: pg link <folder> -p <project>)",
                aside.display()
            );
        }
        let mut links = Links::load(home);
        f(&mut links);
        let mut map = serde_json::Map::new();
        for (folder, list) in &links.folders {
            let entries: Vec<Value> = list.iter().map(|(p, used)| json!({"project": p, "used": used})).collect();
            map.insert(folder.clone(), Value::Array(entries));
        }
        files::write_atomic(&dir.join(".links"), serde_json::to_string_pretty(&Value::Object(map))?.as_bytes())
    }

    pub fn load(home: &Home) -> Links {
        let mut folders = Vec::new();
        if let Ok(text) = std::fs::read_to_string(home.projects_dir().join(".links"))
            && let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text)
        {
            for (folder, list) in map {
                let projects = list
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|e| Some((e["project"].as_str()?.to_string(), e["used"].as_i64().unwrap_or(0))))
                    .collect();
                folders.push((folder, projects));
            }
        }
        Links { folders }
    }

    /// The projects of a folder and of the folders above it, most recently
    /// used first.
    pub fn projects_of(&self, folder: &Path) -> Vec<String> {
        let mut all: Vec<(String, i64)> = Vec::new();
        for key in ancestors(folder) {
            if let Some((_, list)) = self.folders.iter().find(|(f, _)| *f == key) {
                // the latest link last in the list wins a tie (same second)
                for (p, used) in list.iter().rev() {
                    if !all.iter().any(|(q, _)| q == p) {
                        all.push((p.clone(), *used));
                    }
                }
            }
        }
        all.sort_by_key(|e| std::cmp::Reverse(e.1));
        all.into_iter().map(|(p, _)| p).collect()
    }

    /// The nearest folder at or above `folder` linked to the project: where
    /// its code is (c:/dev/shop-api for c:/dev/shop-api/src).
    pub fn code_folder(&self, folder: &Path, project: &str) -> Option<String> {
        ancestors(folder)
            .find(|key| self.folders.iter().any(|(f, list)| f == key && list.iter().any(|(p, _)| p == project)))
    }

    /// The names of the linked folders at or above a folder ("shop-api" for
    /// c:/dev/shop-api/src): what an agent may call the project.
    pub fn folder_names(&self, folder: &Path) -> Vec<String> {
        ancestors(folder)
            .filter(|key| self.folders.iter().any(|(f, _)| f == key))
            .filter_map(|key| key.rsplit('/').next().map(str::to_string))
            .collect()
    }
}

/// The keys of a folder and of every folder above it, nearest first.
fn ancestors(folder: &Path) -> impl Iterator<Item = String> + '_ {
    folder.ancestors().map(folder_key)
}

/// Folders compare by their absolute path with '/', case-folded on Windows.
pub fn folder_key(p: &Path) -> String {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let s = abs.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_string();
    if cfg!(windows) { s.to_lowercase() } else { s }
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/state/links.rs"]
mod tests;
