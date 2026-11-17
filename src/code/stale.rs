//! Notes about code that may be outdated: the code they are about is gone,
//! or its signature changed. Within limits: only notes that name their code
//! (a record's `files`, `symbols`, `code_state`), and only those two strong
//! signs. A function whose body changed is how code lives; it marks nothing.
//! A rename is followed (the notes take the new name, code/notes.rs).
//!
//! A record written about code keeps its state then:
//! `code_state: payments/refund.go#Refund=9f3a1c2b/0c41aa07`, the hash of
//! the function's body and of its signature (older records have the body
//! only: they are marked when the code is gone). Records with only a
//! `commit:` are compared with the code at that commit by git.

use super::{CodeIndex, git, lang::Lang, parse};
use crate::project::{Home, Project};
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;

/// Checks notes against the code, opening the code index only when a note
/// has something to check.
pub struct Staleness<'a> {
    home: &'a Home,
    project: &'a Project,
    code: Option<Option<CodeIndex>>,
    /// Files as they were at a commit, parsed once: (commit, folder, path).
    then: HashMap<(String, String, String), Option<Vec<parse::Symbol>>>,
}

/// Why a note may be outdated.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drift {
    /// Places it names that are gone.
    pub gone: Vec<String>,
    /// Functions whose signature changed.
    pub signature: Vec<String>,
}

impl Drift {
    pub fn is_empty(&self) -> bool {
        self.gone.is_empty() && self.signature.is_empty()
    }

    /// "may be outdated: payments/refund.go#Refund is gone; the signature
    /// of tax.go#Rate changed"
    pub fn text(&self) -> String {
        let mut parts = Vec::new();
        if !self.gone.is_empty() {
            parts.push(format!("{} {} gone", self.gone.join(", "), if self.gone.len() == 1 { "is" } else { "are" }));
        }
        if !self.signature.is_empty() {
            parts.push(format!("the signature of {} changed", self.signature.join(", ")));
        }
        format!("may be outdated since it was written: {}", parts.join("; "))
    }
}

/// A `code_state` entry: place, body hash, signature hash (newer records).
fn entry(e: &str) -> Option<(&str, &str, Option<&str>)> {
    let (place, hashes) = e.trim().rsplit_once('=')?;
    let (body, sig) = match hashes.split_once('/') {
        Some((b, s)) => (b, Some(s)),
        None => (hashes, None),
    };
    Some((place, body, sig))
}

fn short(h: u64) -> String {
    format!("{h:08x}").chars().take(8).collect()
}

impl<'a> Staleness<'a> {
    pub fn new(home: &'a Home, project: &'a Project) -> Staleness<'a> {
        Staleness { home, project, code: None, then: HashMap::new() }
    }

    /// The note's drift as a line, or None when its code is all there.
    pub fn check(&mut self, props: &Value) -> Result<Option<String>> {
        Ok(self.drift(props)?.filter(|d| !d.is_empty()).map(|d| d.text()))
    }

    /// What of the code a note names is gone or has another signature;
    /// None when the note names no code (or there is no code index).
    pub fn drift(&mut self, props: &Value) -> Result<Option<Drift>> {
        if let Some(state) = props["code_state"].as_str() {
            return self.against_state(state);
        }
        let Some(commit) = props["commit"].as_str().map(str::to_string) else { return Ok(None) };
        let list = |key: &str| -> Vec<String> {
            props[key].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
        };
        let (files, symbols) = (list("files"), list("symbols"));
        if files.is_empty() && symbols.is_empty() {
            return Ok(None);
        }
        if self.code()?.is_none() {
            return Ok(None);
        }
        let mut d = Drift::default();
        for f in files.iter().filter(|f| !f.ends_with('/')) {
            let code = self.code()?.expect("checked above");
            if code.locate(f)?.is_none() && !self.anywhere(f) {
                d.gone.push(f.clone());
            }
        }
        for s in &symbols {
            let code = self.code()?.expect("checked above");
            let (file, name) = match s.split_once('#') {
                Some((f, n)) => (Some(f), n),
                None => (None, s.as_str()),
            };
            let now: Vec<super::Sym> = match file {
                Some(f) => match code.locate(f)? {
                    Some((folder, path)) => code
                        .symbols_of(&folder, &path)?
                        .into_iter()
                        .filter(|x| x.qualified == name || x.name == name)
                        .collect(),
                    None => Vec::new(),
                },
                None => code.named(name)?,
            };
            let Some(sym) = now.first().cloned() else {
                d.gone.push(s.clone());
                continue;
            };
            let place = format!("{}#{}", code.display(&sym.folder, &sym.path), sym.qualified);
            if let Some(before) = self.at_commit(&commit, &sym.folder, &sym.path)?
                && let Some(b) = before.iter().find(|x| x.qualified == sym.qualified)
                && normalize(&b.signature) != normalize(&sym.signature)
            {
                d.signature.push(place);
            }
        }
        Ok(Some(d))
    }

    /// Whether a file named by a note still exists below a code folder
    /// (left out of the index, yet there).
    fn anywhere(&mut self, file: &str) -> bool {
        let Ok(Some(code)) = self.code() else { return false };
        code.folders.iter().any(|f| f.dir.join(file).is_file())
    }

    /// The symbols of a file as it was at a commit (None: git does not know).
    fn at_commit(&mut self, commit: &str, folder: &str, path: &str) -> Result<Option<Vec<parse::Symbol>>> {
        let key = (commit.to_string(), folder.to_string(), path.to_string());
        if let Some(found) = self.then.get(&key) {
            return Ok(found.clone());
        }
        let found = (|| {
            let code = self.code().ok()??;
            let dir = code.folders.iter().find(|f| f.key == folder).map(|f| f.dir.clone())?;
            let lang = Lang::of(path)?;
            let text = git::at_commit(&dir, commit, path)?;
            Some(parse::symbols(lang, &text))
        })();
        self.then.insert(key, found.clone());
        Ok(found)
    }

    fn code(&mut self) -> Result<Option<&CodeIndex>> {
        if self.code.is_none() {
            self.code = Some(super::notes::open(self.home, self.project)?);
        }
        Ok(self.code.as_ref().and_then(|c| c.as_ref()))
    }

    /// Each "place=body/signature" of a code_state compared with the code now.
    fn against_state(&mut self, state: &str) -> Result<Option<Drift>> {
        let Some(code) = self.code()? else { return Ok(None) };
        let mut d = Drift::default();
        for e in state.split(',').filter_map(entry) {
            let (place, _body, sig) = e;
            match now(code, place)? {
                None => d.gone.push(place.to_string()),
                Some(at) => {
                    if let (Some(then), Some(now_sig)) = (sig, at.signature)
                        && short(now_sig) != then
                    {
                        d.signature.push(place.to_string());
                    }
                }
            }
        }
        Ok(Some(d))
    }
}

/// A signature as it means: the same with other spacing is the same.
fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A place now: its body hash and, for a function, its signature's hash.
struct Now {
    signature: Option<u64>,
}

/// A place now: "file#Symbol" or "file". None when it is gone (deleted; a
/// rename is followed by the notes before this is asked).
fn now(code: &CodeIndex, place: &str) -> Result<Option<Now>> {
    let (file, symbol) = match place.split_once('#') {
        Some((f, s)) => (f, Some(s)),
        None => (place, None),
    };
    let Some((folder, path)) = code.locate(file)? else { return Ok(None) };
    Ok(match symbol {
        Some(s) => code
            .symbols_of(&folder, &path)?
            .into_iter()
            .find(|x| x.qualified == s)
            .map(|x| Now { signature: Some(parse::hash(&normalize(&x.signature))) }),
        None => Some(Now { signature: None }),
    })
}

/// The state of some code now, to keep with a record about it: each named
/// function's body and signature hashes, and the hash of each file with no
/// function named.
pub fn state_of(code: &CodeIndex, files: &[String], symbols: &[String]) -> Result<Option<String>> {
    let mut entries: Vec<String> = Vec::new();
    let mut covered: Vec<String> = Vec::new();
    for s in symbols {
        for x in code.named(s.rsplit('#').next().unwrap_or(s))? {
            let place = format!("{}#{}", code.display(&x.folder, &x.path), x.qualified);
            if entries.iter().any(|e| e.starts_with(&format!("{place}="))) {
                continue; // named twice (its symbols and its code_state)
            }
            let sig = parse::hash(&normalize(&x.signature));
            entries.push(format!("{place}={}/{}", short(x.body_hash), short(sig)));
            covered.push(code.display(&x.folder, &x.path));
        }
    }
    for f in files.iter().filter(|f| !f.ends_with('/')) {
        if let Some((folder, path)) = code.locate(f)?
            && !covered.contains(&code.display(&folder, &path))
            && !entries.iter().any(|e| e.starts_with(&format!("{}=", code.display(&folder, &path))))
        {
            let place = code.display(&folder, &path);
            let dir = code.folders.iter().find(|x| x.key == folder).map(|x| x.dir.clone()).unwrap_or_default();
            if let Ok(t) = std::fs::read_to_string(dir.join(&path)) {
                entries.push(format!("{place}={}", short(parse::hash(&t))));
            }
        }
    }
    Ok((!entries.is_empty()).then(|| entries.join(", ")))
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/code/stale.rs"]
mod tests;
