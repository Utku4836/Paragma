//! pg write: create or update a note, a folder or a project.
//!
//! ```text
//! pg write physics/collisions "Use swept AABB."   note in the current project
//! pg write physics/                               folder
//! pg write game-engine/                           project (when none is current)
//! pg write shop/payments/refunds "..."            note in another project
//! ```

use super::{Context, USAGE, fail};
use crate::index::Index;
use crate::note::{Kind, Note};
use crate::project::{Project, check_name, files};
use crate::state::State;
use anyhow::Result;
use std::io::Read;

#[derive(clap::Args)]
pub struct Args {
    /// Where: note, folder/ or project/ (a name ending in '/' is a folder)
    path: String,

    /// The note's text ("-" reads it from stdin)
    text: Vec<String>,

    /// Add the text at the end instead of replacing the body
    #[arg(long)]
    append: bool,

    /// Read the text from a file
    #[arg(long, value_name = "FILE")]
    file: Option<std::path::PathBuf>,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let text = text_of(&a)?;
    let folder_only = a.path.ends_with('/');
    let (project, rel) = target(cx, a.path.trim_end_matches('/'))?;
    for part in rel.split('/').filter(|s| !s.is_empty()) {
        check_name(part.trim_end_matches(".md")).map_err(|e| fail(USAGE, e.to_string()))?;
    }
    let project = match cx.home.project(&project) {
        Some(p) => p,
        None => cx.home.create_project(&project).map_err(|e| fail(USAGE, e.to_string()))?,
    };
    if rel.is_empty() {
        println!("ok {} (project)", project.name);
        return Ok(());
    }
    if folder_only {
        std::fs::create_dir_all(project.dir.join(&rel))?;
        println!("ok {}/{rel}/ (folder)", project.name);
        return Ok(());
    }
    let (id, what) = write_note(&project, &rel, &text, a.append)?;
    State::open(&project)?.touch_session(&cx.cwd.to_string_lossy())?;
    println!("ok {}/{} ({id}, {what})", project.name, rel.trim_end_matches(".md"));
    Ok(())
}

/// Writes the note under the project's write lock and indexes it at once,
/// so the next command (or a parallel one) sees it.
pub fn write_note(project: &Project, rel: &str, text: &str, append: bool) -> Result<(String, &'static str)> {
    let _lock = files::WriteLock::take(&project.meta_dir())?;
    let mut index = Index::open(project)?;
    let file = project.note_file(rel);
    let rel = project.rel(&file);
    let (mut note, what) = match files::read_text(&file) {
        Ok(old) => {
            let mut n = Note::parse(&old);
            if append {
                let body = n.body.trim_end().to_string();
                n.body = if body.is_empty() { text.to_string() } else { format!("{body}\n\n{text}") };
                (n, "appended")
            } else {
                n.body = text.to_string();
                (n, "updated")
            }
        }
        Err(_) => {
            let mut n = Note::default();
            let title = rel.trim_end_matches(".md").rsplit('/').next().unwrap_or(&rel).to_string();
            n.props.set_text("type", Kind::Note.as_str());
            n.props.set_text("title", title);
            n.body = text.to_string();
            (n, "created")
        }
    };
    let id = match note.id() {
        Some(id) => id.to_string(),
        None => {
            let id = index.next_id(note.kind())?;
            let mut props = crate::note::Props::default();
            props.set_text("id", id.clone());
            for (k, v) in note.props.iter() {
                props.set(k, v.clone());
            }
            note.props = props;
            id
        }
    };
    if !note.body.is_empty() && !note.body.ends_with('\n') {
        note.body.push('\n');
    }
    files::write_atomic(&file, note.render().as_bytes())?;
    index.refresh(project, &rel)?;
    State::open(project)?.event("write", &rel, what)?;
    Ok((id, what))
}

/// The project and the path in it a write is aimed at.
fn target(cx: &Context, path: &str) -> Result<(String, String)> {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return Err(fail(USAGE, "say where: pg write <note> \"text\" (or folder/, or project/)"));
    }
    let (first, rest) = path.split_once('/').unwrap_or((path, ""));
    match &cx.project {
        // inside the current project, unless the path names another project
        Some(p) if first != p.name && !rest.is_empty() && cx.home.project(first).is_some() => {
            Ok((first.to_string(), rest.to_string()))
        }
        Some(p) if first == p.name && !rest.is_empty() => Ok((p.name.clone(), rest.to_string())),
        Some(p) => Ok((p.name.clone(), path.to_string())),
        // no current project: the first part is the project
        None => {
            check_name(first).map_err(|e| fail(USAGE, e.to_string()))?;
            Ok((first.to_string(), rest.to_string()))
        }
    }
}

fn text_of(a: &Args) -> Result<String> {
    if let Some(f) = &a.file {
        return Ok(std::fs::read_to_string(f)?);
    }
    if a.text.len() == 1 && a.text[0] == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        return Ok(s);
    }
    Ok(a.text.join(" "))
}
