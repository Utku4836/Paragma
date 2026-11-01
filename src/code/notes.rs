//! Notes tied to code. A note names the code it is about in its properties:
//!
//! ```text
//! files: [payments/refund.go, pricing/]      a file, or every file below a folder
//! symbols: [Refund, Cart.Total]              a function, method or type
//! ```
//!
//! and a rule in its text: "- r3: Refunds only within 30 days. (applies:
//! payments/)". When a symbol is renamed or moved, the notes naming it
//! follow.

use super::{CodeIndex, Rename};
use crate::index::{Index, update_note};
use crate::note::{Kind, Value};
use crate::project::{Home, Project};
use anyhow::Result;

/// A note (or rule) about some code.
#[derive(Clone, Debug, PartialEq)]
pub struct Linked {
    pub id: String,
    pub kind: Kind,
    /// Its first line or rule text.
    pub text: String,
}

/// The project's code index, with notes following renamed symbols.
pub fn open(home: &Home, project: &Project) -> Result<Option<CodeIndex>> {
    let Some((code, renames)) = CodeIndex::open(home, project)? else { return Ok(None) };
    if !renames.is_empty() {
        follow(project, &renames)?;
    }
    Ok(Some(code))
}

/// Every note and rule that names code, read once.
pub struct CodeNotes {
    entries: Vec<(Linked, Vec<String>, Vec<String>)>, // note, files, symbols
}

impl CodeNotes {
    pub fn load(index: &Index) -> Result<CodeNotes> {
        let mut entries = Vec::new();
        for kind in [Kind::Rule, Kind::Decision, Kind::Error, Kind::Note] {
            for n in index.notes_of(kind)? {
                let list = |key: &str| -> Vec<String> {
                    n.props[key]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                };
                let (files, symbols) = (list("files"), list("symbols"));
                if files.is_empty() && symbols.is_empty() {
                    continue;
                }
                let text = n.props["title"].as_str().unwrap_or("").to_string();
                entries.push((Linked { id: n.id.clone(), kind, text }, files, symbols));
            }
        }
        for p in index.pieces_of(Kind::Rule)? {
            if let Some(files) = applies(&p.text) {
                entries.push((Linked { id: p.id, kind: Kind::Rule, text: p.text }, files, Vec::new()));
            }
        }
        Ok(CodeNotes { entries })
    }

    /// Notes and rules about a file, or about a symbol in it.
    pub fn about(&self, path: &str, symbol: Option<&str>) -> Vec<&Linked> {
        self.entries
            .iter()
            .filter(|(_, files, symbols)| {
                files.iter().any(|f| covers(f, path))
                    || symbol.is_some_and(|s| symbols.iter().any(|x| names(x, path, s)))
            })
            .map(|(l, _, _)| l)
            .collect()
    }
}

/// "[2 rules: r1, r3; 1 error: e45]" for notes about some code (ids while
/// they are few); "" for none.
pub fn marks(linked: &[&Linked]) -> String {
    if linked.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    for kind in [Kind::Rule, Kind::Decision, Kind::Error, Kind::Note] {
        let ids: Vec<&str> = linked.iter().filter(|l| l.kind == kind).map(|l| l.id.as_str()).collect();
        if ids.is_empty() {
            continue;
        }
        let n = crate::output::count(ids.len() as i64, kind.as_str());
        parts.push(if ids.len() <= 3 { format!("{n}: {}", ids.join(", ")) } else { n });
    }
    format!("[{}]", parts.join("; "))
}

/// "pricing/" covers every file below it; "refund.go" any file ending so.
fn covers(given: &str, path: &str) -> bool {
    let g = given.trim().trim_start_matches("./");
    if g.is_empty() {
        return false;
    }
    if g.ends_with('/') {
        return path.starts_with(g) || path.contains(&format!("/{g}"));
    }
    path == g || path.ends_with(&format!("/{g}"))
}

/// A `symbols` entry naming a symbol: "Refund", "Cart.Total" or
/// "payments/refund.go#Refund".
fn names(entry: &str, path: &str, symbol: &str) -> bool {
    match entry.split_once('#') {
        Some((f, s)) => covers(f, path) && (s == symbol || symbol.ends_with(&format!(".{s}"))),
        None => entry == symbol || symbol.ends_with(&format!(".{entry}")),
    }
}

/// The paths of "(applies: a/, b.go)" at the end of a rule.
fn applies(text: &str) -> Option<Vec<String>> {
    let (_, rest) = text.rsplit_once("(applies:")?;
    let inside = rest.split(')').next()?;
    Some(inside.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
}

/// Rewrites the `symbols` and `files` of the notes that name a renamed or
/// moved symbol.
fn follow(project: &Project, renames: &[Rename]) -> Result<()> {
    let index = Index::open(project)?;
    for kind in [Kind::Rule, Kind::Decision, Kind::Error, Kind::Note] {
        for n in index.notes_of(kind)? {
            let symbols: Vec<String> = n.props["symbols"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            let hits: Vec<&Rename> =
                renames.iter().filter(|r| symbols.iter().any(|s| names(s, &r.old_path, &r.old))).collect();
            if hits.is_empty() {
                continue;
            }
            update_note(project, &n.path, |note, _, _| {
                let mut symbols = note.props.list("symbols");
                let mut files = note.props.list("files");
                for r in &hits {
                    for s in symbols.iter_mut() {
                        if names(s, &r.old_path, &r.old) {
                            *s = if s.contains('#') { format!("{}#{}", r.new_path, r.new) } else { r.new.clone() };
                        }
                    }
                    for f in files.iter_mut() {
                        if *f == r.old_path {
                            *f = r.new_path.clone();
                        }
                    }
                }
                note.props.set("symbols", Value::List(symbols));
                if !files.is_empty() {
                    note.props.set("files", Value::List(files));
                }
                Ok(())
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_symbols_and_applies() {
        assert!(covers("pricing/", "pricing/vat.go"));
        assert!(covers("refund.go", "payments/refund.go"));
        assert!(!covers("fund.go", "payments/refund.go"));
        assert!(names("Total", "cart/cart.go", "Cart.Total"));
        assert!(names("cart/cart.go#Total", "cart/cart.go", "Cart.Total"));
        assert!(!names("other.go#Total", "cart/cart.go", "Cart.Total"));
        assert_eq!(
            applies("Amounts are int64. (applies: pricing/, payments/)").unwrap(),
            vec!["pricing/", "payments/"]
        );
    }
}
