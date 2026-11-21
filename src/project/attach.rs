//! Files kept with a note (`pg write <note> --file <path>`): a copy next to
//! the note, and a card for it in the note's own "Attachments" section,
//! which pg keeps (the rest of the note is the writer's):
//!
//! ```text
//! ## Attachments
//! - [budget.xlsx](budget.xlsx) · 34 KB · 2026-11-21 · from C:\Users\me\Downloads\budget.xlsx · hash 1a2b3c4d5e6f7a8b
//!   - v1: [budget.v1.xlsx](budget.v1.xlsx) · 30 KB · 2026-11-02 · hash 0f1e2d3c4b5a6978
//! ```
//!
//! The same file again keeps no second copy; a changed one is a new
//! version, the old one kept beside it. A file that holds a key or a
//! password is not kept: memory may go to GitHub.

use super::Project;
use crate::index::update_note;
use crate::note::Note;
use crate::output::{date, now};
use anyhow::{Result, bail};
use std::path::Path;

/// The section of a note pg keeps its cards in.
pub const SECTION: &str = "Attachments";

/// A file kept with a note, as its card says.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    /// The file's name beside the note.
    pub name: String,
    pub size: u64,
    pub date: String,
    pub source: String,
    pub hash: String,
    /// Older versions: name, size, date, hash.
    pub versions: Vec<(String, u64, String, String)>,
}

/// FNV-1a over every byte: the same file is known again.
pub fn hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// "34 KB", "2.1 MB".
pub fn size(n: u64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{} KB", n.div_ceil(1024)),
        n => format!("{:.1} MB", n as f64 / 1024.0 / 1024.0),
    }
}

fn parse_size(s: &str) -> u64 {
    let (n, unit) = s.trim().split_once(' ').unwrap_or((s, "B"));
    let n: f64 = n.parse().unwrap_or(0.0);
    (match unit {
        "KB" => n * 1024.0,
        "MB" => n * 1024.0 * 1024.0,
        _ => n,
    }) as u64
}

/// "[name](name)" -> name.
fn link_name(part: &str) -> Option<String> {
    let (name, _) = part.trim().strip_prefix('[')?.split_once("](")?;
    Some(name.to_string())
}

/// The cards of a note's Attachments section.
pub fn cards(note: &Note) -> Vec<Card> {
    let Some(section) = note.section(SECTION) else { return Vec::new() };
    let mut out: Vec<Card> = Vec::new();
    for line in section.lines() {
        let nested = line.starts_with("  ");
        let Some(rest) = line.trim_start().strip_prefix("- ") else { continue };
        let parts: Vec<&str> = rest.split(" · ").collect();
        let field = |prefix: &str| parts.iter().find_map(|p| p.strip_prefix(prefix)).unwrap_or("").to_string();
        if nested {
            // "v1: [name](name) · 30 KB · date · hash h"
            let first = parts.first().copied().unwrap_or("");
            let Some(name) = link_name(first.split_once(": ").map(|x| x.1).unwrap_or(first)) else { continue };
            if let Some(card) = out.last_mut() {
                card.versions.push((
                    name,
                    parts.get(1).map(|s| parse_size(s)).unwrap_or(0),
                    parts.get(2).unwrap_or(&"").to_string(),
                    field("hash "),
                ));
            }
            continue;
        }
        let Some(name) = parts.first().and_then(|p| link_name(p)) else { continue };
        out.push(Card {
            name,
            size: parts.get(1).map(|s| parse_size(s)).unwrap_or(0),
            date: parts.get(2).unwrap_or(&"").to_string(),
            source: field("from "),
            hash: field("hash "),
            versions: Vec::new(),
        });
    }
    out
}

fn render(cards: &[Card]) -> String {
    let mut lines = Vec::new();
    for c in cards {
        let from = if c.source.is_empty() { String::new() } else { format!(" · from {}", c.source) };
        lines.push(format!("- [{n}]({n}) · {} · {}{from} · hash {}", size(c.size), c.date, c.hash, n = c.name));
        for (i, (name, sz, d, h)) in c.versions.iter().enumerate() {
            lines.push(format!("  - v{}: [{name}]({name}) · {} · {d} · hash {h}", i + 1, size(*sz)));
        }
    }
    lines.join("\n")
}

/// The text a file holds, to look for keys in it: a document's text, a
/// text file's text, nothing for other files.
fn text_of(path: &Path, bytes: &[u8]) -> String {
    if crate::docs::Kind::of(path).is_some() {
        return crate::docs::text(path).unwrap_or_default();
    }
    match std::str::from_utf8(bytes) {
        Ok(t) if !t.contains('\0') => t.to_string(),
        _ => String::new(),
    }
}

/// What was done.
pub enum Kept {
    New { name: String, size: u64 },
    Version { name: String, size: u64, old: String },
    Same { name: String },
}

/// Keeps a copy of `source` next to the note `rel` (made when not there).
pub fn keep(project: &Project, rel: &str, source: &Path) -> Result<Kept> {
    let bytes = std::fs::read(source).map_err(|e| anyhow::anyhow!("cannot read {}: {e}", source.display()))?;
    // a key or a password in it: not kept (memory may go to GitHub)
    let (_, found) = crate::record::secrets::mask(&text_of(source, &bytes));
    if !found.is_empty() {
        bail!(
            "{} holds what looks like a secret ({}): not kept, memory may go to GitHub. Keep the key with pg write --secret \"<title>\" <value>, or a copy of the file without it",
            source.display(),
            found.join(", ")
        );
    }
    let h = hash(&bytes);
    let rel = if rel.ends_with(".md") { rel.to_string() } else { format!("{rel}.md") };
    let folder = Path::new(&rel).parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let dir = project.dir.join(&folder);
    std::fs::create_dir_all(&dir)?;
    let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let from = std::path::absolute(source).unwrap_or_else(|_| source.to_path_buf()).display().to_string();
    let today = date(now());
    let title = Path::new(&rel).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let (_, kept) = update_note(project, &rel, |n, exists, _| {
        if !exists {
            n.props.set_text("type", crate::note::Kind::Note.as_str());
            n.props.set_text("title", title.clone());
        }
        let mut all = cards(n);
        let kept = match all.iter_mut().find(|c| c.name == name) {
            Some(c) if c.hash == h => Kept::Same { name: name.clone() },
            Some(c) => {
                // the one there becomes a version, named by its number
                let k = c.versions.len() + 1;
                let old = match name.rsplit_once('.') {
                    Some((stem, ext)) => format!("{stem}.v{k}.{ext}"),
                    None => format!("{name}.v{k}"),
                };
                std::fs::rename(dir.join(&c.name), dir.join(&old))?;
                c.versions.push((old.clone(), c.size, c.date.clone(), c.hash.clone()));
                std::fs::write(dir.join(&name), &bytes)?;
                c.size = bytes.len() as u64;
                c.date = today.clone();
                c.source = from.clone();
                c.hash = h.clone();
                Kept::Version { name: name.clone(), size: bytes.len() as u64, old }
            }
            None => {
                // a file of that name beside the note, not one pg kept: another name
                let mut file = name.clone();
                let mut k = 2;
                while dir.join(&file).exists() {
                    file = match name.rsplit_once('.') {
                        Some((stem, ext)) => format!("{stem}-{k}.{ext}"),
                        None => format!("{name}-{k}"),
                    };
                    k += 1;
                }
                std::fs::write(dir.join(&file), &bytes)?;
                all.push(Card {
                    name: file.clone(),
                    size: bytes.len() as u64,
                    date: today.clone(),
                    source: from.clone(),
                    hash: h.clone(),
                    versions: Vec::new(),
                });
                Kept::New { name: file, size: bytes.len() as u64 }
            }
        };
        n.set_section(SECTION, &render(&all));
        Ok(kept)
    })?;
    Ok(kept)
}

/// Every file kept with a note in the projects folder: its path under it
/// (project/folder/name, versions too) and size.
pub fn all(projects: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for (rel, text) in super::scan::all_notes(projects) {
        let note = Note::parse(&text);
        let folder = Path::new(&rel).parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        for c in cards(&note) {
            let at = |n: &str| if folder.is_empty() { n.to_string() } else { format!("{folder}/{n}") };
            let real = |n: &str| std::fs::metadata(projects.join(at(n))).map(|m| m.len()).ok();
            if let Some(s) = real(&c.name) {
                out.push((at(&c.name), s));
            }
            for (v, _, _, _) in &c.versions {
                if let Some(s) = real(v) {
                    out.push((at(v), s));
                }
            }
        }
    }
    out
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/project/attach.rs"]
mod tests;
