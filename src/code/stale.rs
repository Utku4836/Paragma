//! Notes about code that changed since they were written. A note written
//! with the code's commit (`commit:`) and the code it is about (`files:`,
//! `symbols:`) is checked against the code now: when the lines of its
//! functions changed (or many lines of its files), output says so, because
//! a note that was right then may be wrong now.

use super::{CodeIndex, git};
use crate::project::{Home, Project};
use anyhow::Result;
use serde_json::Value;

/// Lines of a file (with no function named) that must change to call a
/// note about the whole file stale.
const FILE_LINES: usize = 10;

/// Checks notes against the code, opening the code index only when a note
/// has something to check.
pub struct Staleness<'a> {
    home: &'a Home,
    project: &'a Project,
    code: Option<Option<CodeIndex>>,
}

impl<'a> Staleness<'a> {
    pub fn new(home: &'a Home, project: &'a Project) -> Staleness<'a> {
        Staleness { home, project, code: None }
    }

    /// "code changed since it was written: payments/refund.go#Refund", or
    /// None when it did not (or there is nothing to check).
    pub fn check(&mut self, props: &Value) -> Result<Option<String>> {
        let Some(commit) = props["commit"].as_str() else { return Ok(None) };
        let list = |key: &str| -> Vec<String> {
            props[key].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
        };
        let (files, symbols) = (list("files"), list("symbols"));
        if files.is_empty() && symbols.is_empty() {
            return Ok(None);
        }
        if self.code.is_none() {
            self.code = Some(super::notes::open(self.home, self.project)?);
        }
        let Some(Some(code)) = &self.code else { return Ok(None) };

        // the files to look at, with the functions named in each
        let mut places: Vec<(String, String, Vec<String>)> = Vec::new();
        for f in files.iter().filter(|f| !f.ends_with('/')) {
            if let Some((folder, path)) = code.locate(f)? {
                places.push((folder, path, Vec::new()));
            }
        }
        for s in &symbols {
            let (file, name) = match s.split_once('#') {
                Some((f, n)) => (Some(f), n),
                None => (None, s.as_str()),
            };
            let found = match file {
                Some(f) => code.locate(f)?.into_iter().collect::<Vec<_>>(),
                None => code.named(name)?.into_iter().map(|x| (x.folder, x.path)).collect(),
            };
            for (folder, path) in found {
                match places.iter_mut().find(|p| p.0 == folder && p.1 == path) {
                    Some(p) => p.2.push(name.to_string()),
                    None => places.push((folder, path, vec![name.to_string()])),
                }
            }
        }

        let mut changed = Vec::new();
        for (folder, path, names) in places {
            let Some(dir) = code.folders.iter().find(|f| f.key == folder).map(|f| f.dir.clone()) else { continue };
            let Some(hunks) = git::changed_since(&dir, commit, &path) else { continue };
            if hunks.is_empty() {
                continue;
            }
            let shown = code.display(&folder, &path);
            if names.is_empty() {
                let lines: usize = hunks.iter().map(|(a, b)| b - a + 1).sum();
                if lines >= FILE_LINES {
                    changed.push(format!("{shown} ({lines} lines)"));
                }
                continue;
            }
            for s in code.symbols_of(&folder, &path)? {
                let named = names.iter().any(|n| s.qualified == *n || s.name == *n || s.qualified.ends_with(&format!(".{n}")));
                if named && hunks.iter().any(|(a, b)| s.start <= *b && s.end >= *a) {
                    changed.push(format!("{shown}#{}", s.qualified));
                }
            }
        }
        Ok((!changed.is_empty()).then(|| format!("code changed since it was written: {}", changed.join(", "))))
    }
}
