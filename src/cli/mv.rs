//! pg mv (also pg rename): rename or move a note, a folder or a project.
//!
//! ```text
//! pg mv notes/provider payments/provider     a note, to another name or folder
//! pg mv e45 errors/old/                      a note by id, into a folder
//! pg mv physics/ engine/                     a folder, into another one
//! pg mv old-name new-name                    a project
//! pg mv shop/notes/x billing/notes/          to another project
//! pg mv .archive/notes/x notes/x             back from the archive (see pg rm)
//! ```
//!
//! Ids stay (a note moved into a project that already uses its id gets a
//! new one, and says so), and [[links]] to a renamed note follow it.

use super::rm::rename;
use super::target::{Target, resolve};
use super::{Context, USAGE, fail};
use crate::index::{Index, update_note};
use crate::note::Note;
use crate::project::{Project, check_name, files, scan};
use crate::state::State;
use crate::state::links::Links;
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// What to move: a note (id or path), a folder/ or a project
    from: String,

    /// Where to: a new name or path; a folder/ to move into
    to: String,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let (project, rel, folder) = match resolve(cx, &a.from)? {
        Target::Project(p) => return rename_project(cx, &p, a.to.trim_matches('/')),
        Target::Rule { id, .. } => {
            return Err(fail(
                USAGE,
                format!("{id} is a line of rules.md: reword it with pg write --rule, remove it with pg rm {id}"),
            ));
        }
        Target::Note { project, rel } => (project, rel, false),
        Target::Folder { project, rel } => (project, rel, true),
    };
    let (dest, mut to) = destination(cx, &project, &a.to);
    // into a folder: keep the name
    let into = a.to.ends_with('/') || dest.dir.join(&to).is_dir();
    let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    if into {
        to = if to.is_empty() { name.clone() } else { format!("{to}/{name}") };
    } else if !folder && !to.ends_with(".md") {
        to.push_str(".md");
    }
    for part in to.split('/').filter(|p| *p != super::rm::ARCHIVE) {
        check_name(part.trim_end_matches(".md")).map_err(|e| fail(USAGE, e.to_string()))?;
    }
    if dest.dir.join(&to).exists() {
        return Err(fail(USAGE, format!("{}/{} already exists", dest.name, to.trim_end_matches(".md"))));
    }

    {
        // both projects' write locks, always in the same order
        let mut locked = vec![&project, &dest];
        locked.sort_by(|x, y| x.name.cmp(&y.name));
        locked.dedup_by(|x, y| x.name == y.name);
        let _locks = locked.iter().map(|p| files::WriteLock::take(&p.meta_dir())).collect::<Result<Vec<_>>>()?;
        // the notes as they are before the move: what each link names
        let before: Vec<String> = scan::notes(&project.dir).into_iter().map(|f| f.rel).collect();
        std::fs::create_dir_all(dest.dir.join(&to).parent().unwrap_or(&dest.dir))?;
        rename(&project.dir.join(&rel), &dest.dir.join(&to))?;
        if dest.name == project.name {
            let moved: Vec<(String, String)> = if folder {
                let from = format!("{}/", rel.trim_end_matches('/'));
                before
                    .iter()
                    .filter_map(|p| {
                        p.strip_prefix(&from).map(|rest| (p.clone(), format!("{}/{rest}", to.trim_end_matches('/'))))
                    })
                    .collect()
            } else {
                vec![(rel.clone(), to.clone())]
            };
            follow_links(&project, &before, &moved)?;
        }
    }
    let mut notes = Vec::new();
    if dest.name != project.name {
        notes = fresh_ids(&dest, &to, folder)?;
        Index::open(&project)?;
    }
    Index::open(&dest)?;
    State::open(&dest)?.record(crate::state::journal::Did::Mv { from: rel.to_string(), to: to.to_string() })?;
    let show =
        |p: &Project, r: &str| format!("{}/{}{}", p.name, r.trim_end_matches(".md"), if folder { "/" } else { "" });
    println!("ok {} -> {}", show(&project, &rel), show(&dest, &to));
    for n in notes {
        println!("{n}");
    }
    Ok(())
}

/// The project and path a destination names: "billing/notes/x" is in the
/// billing project when there is one (and it is not a folder here).
fn destination(cx: &Context, from: &Project, to: &str) -> (Project, String) {
    let to = to.trim_matches('/');
    if let Some((first, rest)) = to.split_once('/').or(Some((to, "")))
        && first != from.name
        && !from.dir.join(first).exists()
        && let Some(p) = cx.home.project(first)
    {
        return (p, rest.to_string());
    }
    (from.clone(), to.to_string())
}

fn rename_project(cx: &Context, p: &Project, to: &str) -> Result<()> {
    check_name(to).map_err(|e| fail(USAGE, e.to_string()))?;
    if cx.home.project(to).is_some() {
        return Err(fail(USAGE, format!("project {to} already exists")));
    }
    // not under the project's lock: Windows does not move a folder with an
    // open file in it (rename retries while another pg holds one)
    rename(&p.dir, &cx.home.projects_dir().join(to))?;
    Links::rename_project(&cx.home, &p.name, to)?;
    let renamed = cx.home.project(to).expect("just renamed");
    // the env file is named after the project
    let old_env = renamed.dir.join(format!("{}.env", p.name));
    if old_env.exists() {
        rename(&old_env, &crate::project::env::path(&renamed))?;
    }
    if renamed.note_file(crate::project::plan::FILE).exists() {
        update_note(&renamed, crate::project::plan::FILE, |n, _, _| {
            n.props.set_text("title", to);
            Ok(())
        })?;
    }
    println!("ok project {} -> {to}", p.name);
    Ok(())
}

/// The links to moved notes follow them, in every note of the project:
/// [[folder/name]] gets the new path, [[name]] the new name; a #section or
/// |label stays. What a link names is worked out as it was before the move
/// (search/graph.rs resolve). Runs under the project's write lock.
fn follow_links(project: &Project, before: &[String], moved: &[(String, String)]) -> Result<()> {
    let paths: Vec<&str> = before.iter().map(String::as_str).collect();
    for f in scan::notes(&project.dir) {
        let path = project.dir.join(&f.rel);
        let Ok(text) = files::read_text(&path) else { continue };
        if !text.contains("[[") {
            continue;
        }
        // where this note was: what its links were resolved from
        let was = moved.iter().find(|(_, new)| *new == f.rel).map(|(old, _)| old.as_str()).unwrap_or(&f.rel);
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(i) = rest.find("[[") {
            let after = &rest[i + 2..];
            let Some(j) = after.find("]]") else { break };
            out.push_str(&rest[..i + 2]);
            let inner = &after[..j];
            let cut = inner.find(['|', '#', '^']).unwrap_or(inner.len());
            let (target, tail) = inner.split_at(cut);
            let new_target = crate::search::graph::resolve(target.trim(), was, &paths)
                .and_then(|p| moved.iter().find(|(old, _)| *old == p))
                .map(|(_, new)| {
                    let new = new.trim_end_matches(".md");
                    if target.contains('/') {
                        new.to_string()
                    } else {
                        new.rsplit('/').next().unwrap_or(new).to_string()
                    }
                });
            out.push_str(new_target.as_deref().unwrap_or(target));
            out.push_str(tail);
            out.push_str("]]");
            rest = &after[j + 2..];
        }
        out.push_str(rest);
        if out != text {
            files::write_atomic(&path, out.as_bytes())?;
        }
    }
    Ok(())
}

/// Notes moved into another project keep their ids unless the project
/// already uses them.
fn fresh_ids(dest: &Project, to: &str, folder: bool) -> Result<Vec<String>> {
    let moved: Vec<String> = if folder {
        scan::notes(&dest.dir.join(to)).into_iter().map(|f| format!("{to}/{}", f.rel)).collect()
    } else {
        vec![to.to_string()]
    };
    let index = Index::open(dest)?;
    let mut out = Vec::new();
    for rel in moved {
        let Ok(text) = files::read_text(&dest.dir.join(&rel)) else { continue };
        let note = Note::parse(&text);
        let Some(id) = note.id().map(str::to_string) else { continue };
        let taken =
            index.db.query_row("SELECT COUNT(*) FROM notes WHERE id = ?1 AND path != ?2", [&id, &rel], |r| {
                r.get::<_, i64>(0)
            })?;
        if taken > 0 {
            let (new, _) = update_note(dest, &rel, |n, _, _| {
                n.props.remove("id"); // update_note gives it the next free one
                Ok(())
            })?;
            out.push(format!("{id} was taken in {}: {} is now {new}", dest.name, rel.trim_end_matches(".md")));
        }
    }
    Ok(out)
}
