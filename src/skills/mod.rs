//! What agents are told. The core goes into each agent's instructions file
//! through `pg setup` and is read in every conversation: it is short and
//! says when to use pg. The skills are read when needed with `pg skill
//! <name>`; a copy of each is kept in `<home>/skills/paragma-<name>/SKILL.md`
//! so it can be linked into an agent's own skills folder.

use crate::project::{Home, files};
use anyhow::Result;

pub const CORE: &str = include_str!("core.md");

/// (name, text)
pub const SKILLS: &[(&str, &str)] = &[
    ("coding", include_str!("coding.md")),
    ("projects", include_str!("projects.md")),
    ("knowledge", include_str!("knowledge.md")),
];

const START: &str = "<!-- paragma:start -->";
const END: &str = "<!-- paragma:end -->";

/// The section written into an agent's instructions file.
pub fn section() -> String {
    format!("{START}\n## Memory (Paragma)\n\n{CORE}{END}\n")
}

/// The lines earlier versions of pg wrote in their section: a line inside
/// the section that is neither one of these nor one of this version's is
/// the user's own.
const EARLIER: &str = include_str!("earlier.md");
const HEADING: &str = "## Memory (Paragma)";

/// What writing the section did to a file.
#[derive(Debug, PartialEq)]
pub enum Written {
    /// It was there as this version writes it.
    Unchanged,
    /// Written; the user's own lines found inside the old section (this
    /// many) were kept, just below it.
    Written { kept: usize },
}

/// Writes (or replaces) Paragma's section in an instructions file, keeping
/// the rest of it: only pg's own lines are replaced, never the user's.
pub fn install(path: &std::path::Path) -> Result<Written> {
    let old = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let Some((text, kept)) = with_section(&old) else { return Ok(Written::Unchanged) };
    files::write_atomic(path, text.as_bytes())?;
    Ok(Written::Written { kept })
}

/// The text with this version's section in it, and how many of the user's
/// lines were found inside the old one; None when it is there already.
fn with_section(old: &str) -> Option<(String, usize)> {
    let section = section();
    let Some((i, end)) = place(old) else {
        let text = if old.trim().is_empty() { section } else { format!("{}\n\n{section}", old.trim_end()) };
        return Some((text, 0));
    };
    if old[i..end] == section {
        return None;
    }
    // what the user wrote inside the section goes just below it, as written
    let theirs: Vec<&str> = old[i..end].lines().filter(|l| !l.trim().is_empty() && !ours(l)).collect();
    let mut text = format!("{}{section}", &old[..i]);
    if !theirs.is_empty() {
        text.push('\n');
        for l in &theirs {
            text.push_str(l);
            text.push('\n');
        }
    }
    // a blank line before what follows, as Markdown wants
    if !old[end..].is_empty() && !old[end..].starts_with(['\n', '\r']) {
        text.push('\n');
    }
    text.push_str(&old[end..]);
    (text != old).then_some((text, theirs.len()))
}

/// Where Paragma's section is: between its marks, or, written without
/// them (by hand, or the marks lost), from its heading to the next heading.
/// The end includes the line break after it.
fn place(text: &str) -> Option<(usize, usize)> {
    let line_end = |at: usize| text[at..].find('\n').map_or(text.len(), |n| at + n + 1);
    if let (Some(i), Some(j)) = (text.find(START), text.find(END))
        && j > i
    {
        return Some((i, line_end(j)));
    }
    let mut offset = 0;
    let mut start = None;
    for line in text.split_inclusive('\n') {
        let t = line.trim_end();
        match start {
            None if t == HEADING => start = Some(offset),
            Some(s) if t.starts_with("# ") || t.starts_with("## ") => return Some((s, offset)),
            _ => {}
        }
        offset += line.len();
    }
    start.map(|s| (s, text.len()))
}

/// A line pg writes in its section, this version or an earlier one.
fn ours(line: &str) -> bool {
    let t = line.trim();
    [START, END, HEADING].contains(&t) || CORE.lines().chain(EARLIER.lines()).any(|l| l.trim() == t)
}

/// Whether an instructions file has Paragma's section (any version).
pub fn installed(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|t| place(&t).is_some())
}

/// Keeps `<home>/skills/paragma-<name>/SKILL.md` equal to this version.
pub fn write_copies(home: &Home) -> Result<()> {
    for (name, text) in SKILLS {
        let path = home.root.join("skills").join(format!("paragma-{name}")).join("SKILL.md");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(*text) {
            files::write_atomic(&path, text.as_bytes())?;
        }
    }
    Ok(())
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/skills/mod.rs"]
mod tests;
