//! pg read: print a note.

use super::{Context, NOT_FOUND, fail};
use crate::index::{Index, NoteRow};
use crate::note::{Note, Value, chunks};
use crate::output::Budget;
use crate::project::{Project, files};
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// A note id (e45), a rule id (r3), or a path (physics/collisions, or project/path)
    target: String,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let (project, row) = locate(cx, &a.target)?;
    let note = Note::parse(&files::read_text(&project.dir.join(&row.path))?);
    let mut b = Budget::new(cx.budget);

    // a rule id: just that rule
    let id = a.target.rsplit(':').next().unwrap_or(&a.target);
    if row.id != id
        && let Some(c) = chunks(&note, &row.path).into_iter().find(|c| c.id == id)
    {
        b.add(&format!("[{id} rule] {}#{}\n{}", row.path.trim_end_matches(".md"), c.heading, c.text));
        print!("{}", b.out);
        return Ok(());
    }
    b.add(&format!(
        "[{} {}] {}",
        if row.id.is_empty() { "-" } else { &row.id },
        row.kind.as_str(),
        row.path.trim_end_matches(".md")
    ));
    for (k, v) in note.props.iter() {
        if matches!(k.as_str(), "id" | "type" | "title") {
            continue;
        }
        let v = match v {
            Value::Text(s) => s.clone(),
            Value::List(l) => l.join(", "),
        };
        if !v.is_empty() {
            b.add(&format!("{k}: {v}"));
        }
    }
    if !b.add(note.body.trim()) {
        // too long for the budget: as much as fits, then where the rest is
        let mut lines = note.body.lines();
        let mut n = 0;
        for l in lines.by_ref() {
            if !b.add(l) {
                break;
            }
            n += 1;
        }
        b.out.push_str(&format!("... more below line {n}: pg read {} --budget {}\n", a.target, cx.budget * 2));
    }
    print!("{}", b.out);
    Ok(())
}

/// The note a target names: "e45", "shop:e45", "physics/collisions" or
/// "shop/physics/collisions".
pub fn locate(cx: &Context, target: &str) -> Result<(Project, NoteRow)> {
    let (proj_name, rest) = match target.split_once(':') {
        Some((p, r)) if cx.home.project(p).is_some() => (Some(p.to_string()), r.to_string()),
        _ => (None, target.to_string()),
    };
    let mut candidates: Vec<(Project, String)> = Vec::new();
    match (&proj_name, &cx.project) {
        (Some(p), _) => candidates.push((cx.home.project(p).expect("checked above"), rest.clone())),
        (None, _) => {
            // "shop/physics/collisions": the first part may be a project
            if let Some((first, inner)) = rest.split_once('/')
                && let Some(p) = cx.home.project(first)
            {
                candidates.push((p, inner.to_string()));
            }
            if let Some(p) = &cx.project {
                candidates.push((p.clone(), rest.clone()));
            }
            if cx.project.is_none() {
                for p in cx.home.projects() {
                    candidates.push((p, rest.clone()));
                }
            }
        }
    }
    for (p, r) in candidates {
        let index = Index::open(&p)?;
        let path = if r.ends_with(".md") { r.clone() } else { format!("{r}.md") };
        let row = match index.note_by_path(&path)? {
            Some(row) => Some(row),
            None => index.note_by_ref(&r)?,
        };
        if let Some(row) = row {
            return Ok((p, row));
        }
    }
    Err(fail(NOT_FOUND, format!("no note {target:?} (pg ls lists notes, pg find searches them)")))
}
