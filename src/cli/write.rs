//! pg write: create or update a note, a folder or a project.
//!
//! ```text
//! pg write physics/collisions "Use swept AABB."   note in the current project
//! pg write physics/                               folder
//! pg write game-engine/                           project (when none is current)
//! pg write shop/payments/refunds "..."            note in another project
//!
//! pg write --rule "Amounts are int64 kuruş, never float" --group Money
//! pg write --decision "Discounts apply before VAT" --why "finance asked"
//! pg write --error "catalog checksum mismatch" --cause "CRLF on Windows" --fix "normalise line endings"
//! ```
//!
//! Rules, decisions and errors go to their place in the project with their
//! fields, and are never written twice (see record/).

use super::{Context, USAGE, fail};
use crate::index::update_note;
use crate::note::Kind;
use crate::project::{Project, check_name};
use crate::record::{self, DecisionInput, ErrorInput, Written};
use crate::state::State;
use anyhow::Result;
use std::io::Read;

#[derive(clap::Args)]
pub struct Args {
    /// Where: note, folder/ or project/ (a name ending in '/' is a folder)
    path: Option<String>,

    /// The note's text ("-" reads it from stdin)
    text: Vec<String>,

    /// Add the text at the end instead of replacing the body
    #[arg(long)]
    append: bool,

    /// Read the text from a file
    #[arg(long, value_name = "FILE")]
    file: Option<std::path::PathBuf>,

    /// Record a rule (to rules.md)
    #[arg(long, value_name = "TEXT", conflicts_with_all = ["decision", "error"])]
    rule: Option<String>,

    /// The rule's group in rules.md (default: General)
    #[arg(long, requires = "rule")]
    group: Option<String>,

    /// Record a decision (to decisions/)
    #[arg(long, value_name = "TEXT", conflicts_with = "error")]
    decision: Option<String>,

    /// Why it was decided
    #[arg(long, requires = "decision")]
    why: Option<String>,

    /// What else was considered
    #[arg(long, requires = "decision")]
    alternatives: Option<String>,

    /// The decision this one replaces (its id)
    #[arg(long, value_name = "ID", requires = "decision")]
    supersedes: Option<String>,

    /// A short title (default: the first sentence)
    #[arg(long)]
    title: Option<String>,

    /// Record an error: the error line as printed (to errors/)
    #[arg(long, value_name = "LINE")]
    error: Option<String>,

    /// What caused the error
    #[arg(long, requires = "error")]
    cause: Option<String>,

    /// How it was fixed
    #[arg(long, requires = "error")]
    fix: Option<String>,

    /// Files the error is about, comma separated
    #[arg(long, value_delimiter = ',', requires = "error")]
    files: Vec<String>,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    if a.rule.is_some() || a.decision.is_some() || a.error.is_some() {
        return write_record(cx, &a);
    }
    let Some(path) = a.path.as_deref() else {
        return Err(fail(USAGE, "say where: pg write <note> \"text\" (or --rule, --decision, --error)"));
    };
    let text = text_of(&a)?;
    let folder_only = path.ends_with('/');
    let (project, rel) = target(cx, path.trim_end_matches('/'))?;
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

/// A rule, a decision or an error, put in its place by record/.
fn write_record(cx: &Context, a: &Args) -> Result<()> {
    if a.path.is_some() || !a.text.is_empty() {
        return Err(fail(
            USAGE,
            "a rule, decision or error goes to its own place: give its text to --rule, --decision or --error",
        ));
    }
    let project = cx.need_project()?;
    let usage = |e: anyhow::Error| fail(USAGE, e.to_string());
    let (kind, w): (&str, Written) = if let Some(text) = &a.rule {
        ("rule", record::rule(project, text, a.group.as_deref())?)
    } else if let Some(text) = &a.decision {
        let d = DecisionInput {
            text,
            title: a.title.as_deref(),
            why: a.why.as_deref(),
            alternatives: a.alternatives.as_deref(),
            supersedes: a.supersedes.as_deref(),
        };
        ("decision", record::decision(project, &d).map_err(usage)?)
    } else {
        let line = a.error.as_deref().unwrap_or_default();
        let e = ErrorInput { line, cause: a.cause.as_deref(), fix: a.fix.as_deref(), files: a.files.clone() };
        ("error", record::error(project, &e).map_err(usage)?)
    };
    let state = State::open(project)?;
    state.event("write", kind, &format!("{} {}", w.id, w.what))?;
    state.touch_session(&cx.cwd.to_string_lossy())?;
    println!("ok {} ({kind} {})", w.id, w.what);
    for n in &w.notes {
        println!("{n}");
    }
    Ok(())
}

/// Writes a plain note (replacing its body, or appending to it).
pub fn write_note(project: &Project, rel: &str, text: &str, append: bool) -> Result<(String, &'static str)> {
    let (id, what) = update_note(project, rel, |n, exists, _| {
        if !exists {
            let title = rel.trim_end_matches(".md").rsplit('/').next().unwrap_or(rel).to_string();
            n.props.set_text("type", Kind::Note.as_str());
            n.props.set_text("title", title);
            n.body = text.to_string();
            return Ok("created");
        }
        if append {
            let body = n.body.trim_end().to_string();
            n.body = if body.is_empty() { text.to_string() } else { format!("{body}\n\n{text}") };
            Ok("appended")
        } else {
            n.body = text.to_string();
            Ok("updated")
        }
    })?;
    State::open(project)?.event("write", &project.rel(&project.note_file(rel)), &format!("{id} {what}"))?;
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
