//! pg ls: what is where.
//!
//! Without a path: the projects. With a project or a folder: its folders
//! and notes, each note with its id, type and first line.

use super::{Context, NOT_FOUND, fail};
use crate::index::Index;
use crate::note::Note;
use crate::output::{Budget, count, date, one_line};
use crate::project::{Project, files, scan};
use crate::state::links::Links;
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// A project, or a folder in it (project/folder)
    path: Option<String>,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let (project, folder) = match (&a.path, &cx.project) {
        (None, None) => return list_projects(cx),
        (None, Some(p)) => (p.clone(), String::new()),
        (Some(path), current) => {
            let path = path.trim_matches('/');
            let (first, rest) = path.split_once('/').unwrap_or((path, ""));
            match cx.home.project(first) {
                Some(p) if current.as_ref().is_none_or(|c| !c.dir.join(first).is_dir()) => (p, rest.to_string()),
                _ => match current {
                    Some(c) => (c.clone(), path.to_string()),
                    None => {
                        return Err(fail(NOT_FOUND, format!("no project {first:?} (pg ls lists them)")));
                    }
                },
            }
        }
    };
    list_folder(cx, &project, &folder)
}

fn list_projects(cx: &Context) -> Result<()> {
    let projects = cx.home.projects();
    if projects.is_empty() {
        println!("no projects yet: pg open <name> starts one");
        return Ok(());
    }
    let mut b = Budget::new(cx.budget);
    b.add(&format!("ok {}", count(projects.len() as i64, "project")));
    for p in projects {
        let notes = scan::notes(&p.dir);
        let last = notes.iter().map(|n| n.mtime).max().map(|ns| date(ns / 1_000_000_000)).unwrap_or_default();
        b.add(&format!(
            "{}  {}{}",
            p.name,
            count(notes.len() as i64, "note"),
            if last.is_empty() { String::new() } else { format!(", last change {last}") }
        ));
    }
    print!("{}", b.out);
    Ok(())
}

fn list_folder(cx: &Context, project: &Project, folder: &str) -> Result<()> {
    let dir = project.dir.join(folder);
    if !dir.is_dir() {
        return Err(fail(NOT_FOUND, format!("no folder {folder:?} in {} (pg ls {})", project.name, project.name)));
    }
    let index = Index::open(project)?;
    let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
    let mut b = Budget::new(cx.budget);
    let here = if folder.is_empty() { project.name.clone() } else { format!("{}/{folder}", project.name) };
    b.add(&format!("ok {here}"));
    if folder.is_empty() {
        if let Some(s) = crate::project::env::summary(project) {
            b.add(&s);
        }
        let code = Links::load(&cx.home).folders_of(&project.name);
        if !code.is_empty() {
            b.add(&format!("code: {}", code.join(", ")));
        }
    }
    for sub in scan::folders(&dir) {
        if !sub.contains('/') {
            let n = scan::notes(&dir.join(&sub)).len();
            b.add(&format!("{sub}/  {}", count(n as i64, "note")));
        }
    }
    for row in index.notes()? {
        let Some(name) = row.path.strip_prefix(&prefix) else {
            continue;
        };
        if name.contains('/') {
            continue;
        }
        let first = files::read_text(&project.dir.join(&row.path))
            .map(|t| one_line(&Note::parse(&t).body, 70))
            .unwrap_or_default();
        let id = if row.id.is_empty() { "-".to_string() } else { row.id.clone() };
        b.add(&format!("{}  {id} {}  {first}", name.trim_end_matches(".md"), row.kind.as_str()));
    }
    print!("{}", b.out);
    Ok(())
}
