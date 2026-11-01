//! What a path or id given to `mv` and `rm` names: a note, a rule, a
//! folder or a whole project.

use super::{Context, NOT_FOUND, fail, read};
use crate::project::Project;
use anyhow::Result;

pub enum Target {
    Note { project: Project, rel: String },
    Rule { project: Project, rel: String, id: String },
    Folder { project: Project, rel: String },
    Project(Project),
}

pub fn resolve(cx: &Context, given: &str) -> Result<Target> {
    let s = given.trim().trim_matches('/');
    if is_id(s.rsplit(':').next().unwrap_or(s)) {
        let (project, row) = read::locate(cx, s)?;
        let id = s.rsplit(':').next().unwrap_or(s).to_string();
        return Ok(if row.id != id && id.starts_with('r') {
            Target::Rule { project, rel: row.path, id }
        } else {
            Target::Note { project, rel: row.path }
        });
    }
    if let Some(p) = &cx.project
        && let Some(t) = inside(p, s)
    {
        return Ok(t);
    }
    let (first, rest) = s.split_once('/').unwrap_or((s, ""));
    if let Some(p) = cx.home.project(first) {
        if rest.is_empty() {
            return Ok(Target::Project(p));
        }
        if let Some(t) = inside(&p, rest) {
            return Ok(t);
        }
    }
    Err(fail(NOT_FOUND, format!("nothing called {given:?} (pg ls shows what is where)")))
}

/// A note or folder at `rel` in the project.
fn inside(p: &Project, rel: &str) -> Option<Target> {
    if rel.is_empty() {
        return None;
    }
    if p.dir.join(rel).is_dir() {
        return Some(Target::Folder { project: p.clone(), rel: rel.to_string() });
    }
    let file = p.note_file(rel);
    file.is_file().then(|| Target::Note { project: p.clone(), rel: p.rel(&file) })
}

/// e45, r3, d7, n88, p1.
fn is_id(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some('r' | 'd' | 'e' | 'n' | 'p')) && s.len() > 1 && c.all(|c| c.is_ascii_digit())
}
