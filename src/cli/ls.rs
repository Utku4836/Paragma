//! pg ls: what is where.
//!
//! Without a path: the projects (in a project: its notes). With a project
//! or a folder of notes: its folders and notes, each note with its id,
//! type and first line. With a folder or a file of the code (or --code):
//! the map of the code, its files and their functions, the notes about
//! them marked. --todo: the to-dos of every note; --due: the due dates.

use super::{Context, NOT_FOUND, fail};
use crate::index::Index;
use crate::note::Note;
use crate::output::{Budget, count, date, one_line};
use crate::project::{Project, files, scan};
use crate::state::links::Links;
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// A project, a folder of notes (project/folder), or a folder or file of the code
    path: Option<String>,
    /// The map of the project's code (all of it, or the folder or file given)
    #[arg(long)]
    code: bool,
    /// The to-dos (- [ ] lines) of every note, numbered as pg write --check takes them
    #[arg(long, conflicts_with_all = ["code", "due"])]
    todo: bool,
    /// With --todo: the done ones too
    #[arg(long, requires = "todo")]
    all: bool,
    /// The notes with a due date, the past ones first
    #[arg(long, conflicts_with = "code")]
    due: bool,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    if a.todo {
        return todos(cx, a.all);
    }
    if a.due {
        return dues(cx);
    }
    if a.code {
        return super::tree::run(cx, super::tree::Args { path: a.path });
    }
    // a folder or a file of the code, not of the notes: its map
    if let (Some(path), Some(project)) = (&a.path, &cx.project) {
        let p = path.trim_matches('/');
        let first = p.split('/').next().unwrap_or(p);
        let memory = cx.home.project(first).is_some()
            || project.dir.join(p).exists()
            || project.dir.join(format!("{p}.md")).exists();
        if !memory && is_code(cx, project, p) {
            return super::tree::run(cx, super::tree::Args { path: a.path });
        }
    }
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

/// Whether a path is a folder or file of the project's code.
fn is_code(cx: &Context, project: &Project, path: &str) -> bool {
    if cx.cwd.join(path).exists() {
        return true;
    }
    Links::load(&cx.home).folders_of(&project.name).iter().any(|f| std::path::Path::new(f).join(path).exists())
}

/// Every open to-do of the project's notes (the project note's steps are
/// pg steps), as "n4.2  text  (folder/note)".
fn todos(cx: &Context, all: bool) -> Result<()> {
    let project = cx.need_project()?;
    let index = Index::open(project)?;
    let mut b = Budget::new(cx.budget);
    let mut open = 0;
    let mut lines = Vec::new();
    for row in index.notes()? {
        if row.path == crate::project::plan::FILE {
            continue;
        }
        let Ok(text) = files::read_text(&project.dir.join(&row.path)) else { continue };
        let note = Note::parse(&text);
        let id = if row.id.is_empty() { row.path.trim_end_matches(".md").to_string() } else { row.id.clone() };
        for t in crate::project::items::todos(&note.body) {
            if t.done && !all {
                continue;
            }
            open += usize::from(!t.done);
            lines.push(format!(
                "{} {id}.{}  {}  ({})",
                if t.done { "[x]" } else { "[ ]" },
                t.number,
                one_line(&t.text, 90),
                row.path.trim_end_matches(".md")
            ));
        }
    }
    if lines.is_empty() {
        println!("no to-dos in the notes of {} (a \"- [ ] ...\" line in a note is one)", project.name);
        return Ok(());
    }
    b.add(&format!("to-dos of {}: {open} open (done: pg write --check <id>.<number>)", project.name));
    for l in lines {
        if !b.add(&l) {
            break;
        }
    }
    print!("{}", b.out);
    Ok(())
}

/// The notes with a due date: past, today, coming.
fn dues(cx: &Context) -> Result<()> {
    use crate::project::items::{days_left, when};
    let project = cx.need_project()?;
    let index = Index::open(project)?;
    let mut due: Vec<(i64, String, String, String)> = Vec::new(); // days, date, id, title
    for kind in
        [crate::note::Kind::Note, crate::note::Kind::Decision, crate::note::Kind::Error, crate::note::Kind::Project]
    {
        for n in index.notes_of(kind)? {
            let Some(d) = n.props.get("due").and_then(|v| v.as_str()) else { continue };
            let Some(left) = days_left(d) else { continue };
            let title = n.props.get("title").and_then(|v| v.as_str()).unwrap_or(&n.path).to_string();
            due.push((left, d.to_string(), n.id.clone(), title));
        }
    }
    if due.is_empty() {
        println!("no due dates in {} (pg write <note> --due 2026-12-01 gives one)", project.name);
        return Ok(());
    }
    due.sort();
    let mut b = Budget::new(cx.budget);
    let past = due.iter().filter(|d| d.0 < 0).count();
    b.add(&format!("due dates of {}: {past} past, {} coming", project.name, due.len() - past));
    for (left, date, id, title) in &due {
        b.add(&format!(
            "{} {date} ({})  {id}  {}",
            if *left < 0 { "!" } else { "-" },
            when(*left),
            one_line(title, 80)
        ));
    }
    print!("{}", b.out);
    Ok(())
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
