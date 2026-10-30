//! pg ls: what is where.
//!
//! Without a path: the projects. With a project or a folder: its folders
//! and notes, each note with its id, type and first line.

use super::{Context, NOT_FOUND, fail};
use crate::index::Index;
use crate::note::Note;
use crate::output::{Budget, count, one_line};
use crate::project::{Project, files, scan};
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
        println!("no projects yet: pg write <project>/ creates one");
        return Ok(());
    }
    let mut b = Budget::new(cx.budget);
    b.add(&format!("ok {}", count(projects.len() as i64, "project")));
    for p in projects {
        let notes = scan::notes(&p.dir);
        let last = notes.iter().map(|n| n.mtime).max().map(day).unwrap_or_default();
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

/// A file time as a date.
fn day(nanos: i64) -> String {
    let secs = nanos / 1_000_000_000;
    let days = secs.div_euclid(86_400);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{}-{m:02}-{d:02}", if m <= 2 { y + 1 } else { y })
}

#[cfg(test)]
mod tests {
    #[test]
    fn days() {
        assert_eq!(super::day(0), "1970-01-01");
        assert_eq!(super::day(1_792_000_000 * 1_000_000_000), "2026-10-14");
    }
}
