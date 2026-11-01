//! Notes about code that changed since they were written: a note that was
//! right then may be wrong now, so output says so.
//!
//! A record written about code keeps the state of that code as it was:
//! `code_state: payments/refund.go#Refund=9f3a1c2b` (the hash of the
//! function's body, or of a whole file), taken after the fix the record
//! describes, so a fix does not make its own note stale. Notes with only a
//! `commit:` are checked with git: did the lines of their functions change
//! since that commit?

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
        if let Some(state) = props["code_state"].as_str() {
            return self.against_state(state);
        }
        let Some(commit) = props["commit"].as_str() else { return Ok(None) };
        let list = |key: &str| -> Vec<String> {
            props[key].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
        };
        let (files, symbols) = (list("files"), list("symbols"));
        if files.is_empty() && symbols.is_empty() {
            return Ok(None);
        }
        let Some(code) = self.code()? else { return Ok(None) };

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
                let named =
                    names.iter().any(|n| s.qualified == *n || s.name == *n || s.qualified.ends_with(&format!(".{n}")));
                if named && hunks.iter().any(|(a, b)| s.start <= *b && s.end >= *a) {
                    changed.push(format!("{shown}#{}", s.qualified));
                }
            }
        }
        Ok(said(changed))
    }

    fn code(&mut self) -> Result<Option<&CodeIndex>> {
        if self.code.is_none() {
            self.code = Some(super::notes::open(self.home, self.project)?);
        }
        Ok(self.code.as_ref().and_then(|c| c.as_ref()))
    }

    /// Each "place=hash" of a code_state compared with the code now.
    fn against_state(&mut self, state: &str) -> Result<Option<String>> {
        let Some(code) = self.code()? else { return Ok(None) };
        let mut changed = Vec::new();
        for entry in state.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let Some((place, then)) = entry.rsplit_once('=') else { continue };
            if now(code, place)?.is_some_and(|h| format!("{h:08x}").get(..8) != Some(then)) {
                changed.push(place.to_string());
            }
        }
        Ok(said(changed))
    }
}

fn said(changed: Vec<String>) -> Option<String> {
    (!changed.is_empty()).then(|| format!("code changed since it was written: {}", changed.join(", ")))
}

/// The hash of a place now: "file#Symbol" (its body) or "file" (all of it).
/// None when the place is gone (a rename the notes follow, or a deletion).
fn now(code: &CodeIndex, place: &str) -> Result<Option<u64>> {
    let (file, symbol) = match place.split_once('#') {
        Some((f, s)) => (f, Some(s)),
        None => (place, None),
    };
    let Some((folder, path)) = code.locate(file)? else { return Ok(None) };
    Ok(match symbol {
        Some(s) => code.symbols_of(&folder, &path)?.into_iter().find(|x| x.qualified == s).map(|x| x.body_hash),
        None => {
            let dir = code.folders.iter().find(|f| f.key == folder).map(|f| f.dir.clone()).unwrap_or_default();
            std::fs::read_to_string(dir.join(&path)).ok().map(|t| super::parse::hash(&t))
        }
    })
}

/// The state of some code now, to keep with a record about it: each named
/// function's body hash, and the hash of each file with no function named.
pub fn state_of(code: &CodeIndex, files: &[String], symbols: &[String]) -> Result<Option<String>> {
    let mut entries: Vec<String> = Vec::new();
    let mut covered: Vec<String> = Vec::new();
    for s in symbols {
        for x in code.named(s.rsplit('#').next().unwrap_or(s))? {
            let place = format!("{}#{}", code.display(&x.folder, &x.path), x.qualified);
            entries.push(format!("{place}={:08x}", x.body_hash).chars().take(place.len() + 9).collect());
            covered.push(code.display(&x.folder, &x.path));
        }
    }
    for f in files.iter().filter(|f| !f.ends_with('/')) {
        if let Some((folder, path)) = code.locate(f)?
            && !covered.contains(&code.display(&folder, &path))
            && let Some(h) = now(code, &code.display(&folder, &path))?
        {
            let place = code.display(&folder, &path);
            entries.push(format!("{place}={}", &format!("{h:08x}")[..8]));
        }
    }
    Ok((!entries.is_empty()).then(|| entries.join(", ")))
}
