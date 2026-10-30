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

use crate::project::Home;
use serde_json::Value;
use std::path::Path;

pub struct Links {
    /// folder -> [(project, last used, seconds since the epoch)]
    folders: Vec<(String, Vec<(String, i64)>)>,
}

impl Links {
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

    /// The projects of a folder (or of the nearest linked folder above it),
    /// most recently used first.
    pub fn projects_of(&self, folder: &Path) -> Vec<String> {
        let mut dir = Some(folder);
        while let Some(d) = dir {
            let key = folder_key(d);
            if let Some((_, list)) = self.folders.iter().find(|(f, _)| *f == key) {
                let mut list = list.clone();
                list.sort_by_key(|e| std::cmp::Reverse(e.1));
                return list.into_iter().map(|(p, _)| p).collect();
            }
            dir = d.parent();
        }
        Vec::new()
    }
}

/// Folders compare by their absolute path with '/', case-folded on Windows.
pub fn folder_key(p: &Path) -> String {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let s = abs.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_string();
    if cfg!(windows) { s.to_lowercase() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_and_the_folders_below_it_know_their_project() {
        let d = tempfile::tempdir().unwrap();
        let home = Home { root: d.path().to_path_buf() };
        let api = d.path().join("dev/shop-api");
        let links = serde_json::json!({
            folder_key(&api): [{"project": "old", "used": 1}, {"project": "shop", "used": 2}],
        });
        std::fs::create_dir_all(home.projects_dir()).unwrap();
        std::fs::write(home.projects_dir().join(".links"), links.to_string()).unwrap();
        let links = Links::load(&home);
        assert_eq!(links.projects_of(&api.join("src/handlers")), vec!["shop", "old"]);
        assert!(links.projects_of(&d.path().join("elsewhere")).is_empty());
    }
}
