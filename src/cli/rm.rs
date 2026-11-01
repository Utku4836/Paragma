//! pg rm: take a note, a rule, a folder or a project out of memory.
//!
//! ```text
//! pg rm e45                 a note, by id or path
//! pg rm r3                  one rule (its line leaves rules.md)
//! pg rm physics/            a folder
//! pg rm old-project         a project
//! pg rm e45 --hard          gone for good
//! ```
//!
//! Without --hard nothing is lost: it moves to the project's `.archive/`
//! folder (projects to `projects/.archive/`), which search and `ls` do not
//! look at, and `pg mv` brings it back.

use super::Context;
use super::target::{Target, resolve};
use crate::index::{Index, update_note};
use crate::note::rule_line;
use crate::output::{date, now};
use crate::project::{Project, files};
use crate::state::State;
use crate::state::links::Links;
use anyhow::{Context as _, Result};
use std::path::Path;

#[derive(clap::Args)]
pub struct Args {
    /// A note (id or path), a rule id, a folder/ or a project
    target: String,

    /// Delete for good instead of moving to .archive/
    #[arg(long)]
    hard: bool,
}

pub const ARCHIVE: &str = ".archive";

pub fn run(cx: &Context, a: Args) -> Result<()> {
    match resolve(cx, &a.target)? {
        Target::Rule { project, rel, id } => {
            let mut removed = String::new();
            update_note(&project, &rel, |n, _, _| {
                let mut kept = Vec::new();
                for l in n.body.lines() {
                    match rule_line(l) {
                        Some((rid, text)) if rid == id => removed = text.to_string(),
                        _ => kept.push(l),
                    }
                }
                n.body = kept.join("\n");
                Ok(())
            })?;
            if !a.hard {
                // the archive is not indexed: a plain file, written under the lock
                let _lock = files::WriteLock::take(&project.meta_dir())?;
                let file = project.dir.join(ARCHIVE).join(&rel);
                let old = files::read_text(&file).unwrap_or_else(|_| "# Removed rules\n".to_string());
                let line = format!("- {id}: {removed} (removed {})", date(now()));
                files::write_atomic(&file, format!("{}\n{line}\n", old.trim_end()).as_bytes())?;
            }
            State::open(&project)?.event("rm", &id, "")?;
            println!("ok removed {id}{}", if a.hard { "" } else { " (kept in .archive/rules)" });
        }
        Target::Note { project, rel } => {
            let what = rel.trim_end_matches(".md").to_string();
            take_out(&project, &rel, a.hard)?;
            State::open(&project)?.event("rm", &rel, "")?;
            println!("ok removed {}/{what}{}", project.name, kept_note(&project, &what, a.hard));
        }
        Target::Folder { project, rel } => {
            take_out(&project, &rel, a.hard)?;
            State::open(&project)?.event("rm", &format!("{rel}/"), "")?;
            println!("ok removed {}/{rel}/{}", project.name, kept_note(&project, &rel, a.hard));
        }
        Target::Project(project) => {
            let dir = cx.home.projects_dir();
            if a.hard {
                std::fs::remove_dir_all(&project.dir).context("delete the project")?;
                println!("ok deleted project {}", project.name);
            } else {
                let to = free(&dir.join(ARCHIVE), &project.name);
                std::fs::create_dir_all(dir.join(ARCHIVE))?;
                rename(&project.dir, &to)?;
                println!(
                    "ok removed project {} (kept in projects/{ARCHIVE}/{})",
                    project.name,
                    to.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()
                );
            }
            Links::forget_project(&cx.home, &project.name)?;
        }
    }
    Ok(())
}

/// Moves a note or folder to the project's archive (or deletes it), under
/// the write lock, and drops it from the index.
fn take_out(project: &Project, rel: &str, hard: bool) -> Result<()> {
    let _lock = files::WriteLock::take(&project.meta_dir())?;
    let from = project.dir.join(rel);
    if hard {
        if from.is_dir() { std::fs::remove_dir_all(&from)? } else { std::fs::remove_file(&from)? }
    } else {
        let to = project.dir.join(ARCHIVE).join(rel);
        let to = if to.exists() { free(to.parent().unwrap_or(&project.dir), &name_of(&to)) } else { to };
        std::fs::create_dir_all(to.parent().unwrap_or(&project.dir))?;
        rename(&from, &to)?;
    }
    drop(_lock);
    Index::open(project)?; // brings the index up to date
    Ok(())
}

fn kept_note(project: &Project, rel: &str, hard: bool) -> String {
    if hard {
        String::new()
    } else {
        format!(" (kept in .archive: pg mv {}/{ARCHIVE}/{rel} {rel} brings it back)", project.name)
    }
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `dir/name`, or `dir/name-2` ... when taken (before the extension).
fn free(dir: &Path, name: &str) -> std::path::PathBuf {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    let mut p = dir.join(name);
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem}-{n}{ext}"));
        n += 1;
    }
    p
}

/// A rename that tolerates Windows holding the file for an instant.
pub fn rename(from: &Path, to: &Path) -> Result<()> {
    let mut tries = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(_) if tries < 50 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => return Err(e).with_context(|| format!("move {} to {}", from.display(), to.display())),
        }
    }
}
