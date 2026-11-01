//! pg finish: end this conversation's work on the project.
//!
//! ```text
//! pg finish "Client done, refunds next. Tests pass." --left "Move refunds to the client"
//! ```
//!
//! Ticks off the steps this conversation added (not other conversations'
//! steps), adds the steps left for later, and writes this conversation's
//! handoff on top of the project note: the summary, and what it wrote to
//! memory, the code it changed and how its last test run went. Each
//! conversation has its own handoff; none overwrites another.
//!
//! It also shows the notes about the changed code (and about the code that
//! calls it or that it calls): the rules to check the change against.

use super::{Context, USAGE, fail};
use crate::code::notes::{CodeNotes, Linked};
use crate::code::{CodeIndex, work};
use crate::index::Index;
use crate::index::update_note;
use crate::note::Kind;
use crate::output::{count, date_time, now, one_line};
use crate::project::plan;
use crate::record::upkeep;
use crate::state::State;
use crate::testrun;
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// What was done, and what the next conversation should know
    summary: Vec<String>,

    /// A step left for later (repeat for more)
    #[arg(long = "left", value_name = "STEP")]
    left: Vec<String>,
}

/// Ids of written notes listed in a handoff.
const WRITTEN_SHOWN: usize = 10;
/// Changed symbols named in a handoff.
const CHANGED_SHOWN: usize = 8;
/// Notes about the changed code listed.
const NOTES_SHOWN: usize = 8;

pub fn run(cx: &Context, mut a: Args) -> Result<()> {
    let project = cx.need_project()?;
    let mut summary = a.summary.join(" ").trim().to_string();
    let warning = super::mask_all(std::iter::once(&mut summary).chain(a.left.iter_mut()));
    if summary.is_empty() {
        return Err(fail(USAGE, "say what was done: pg finish \"what was done; what is next\""));
    }
    let state = State::open(project)?;
    let events = state.my_events()?;
    let mine: Vec<&str> = events.iter().filter(|e| e.action == "step").map(|e| e.target.as_str()).collect();
    let mut written: Vec<&str> = Vec::new();
    for e in events.iter().filter(|e| e.action == "write") {
        let id = e.detail.split(' ').next().unwrap_or("");
        if !id.is_empty() && !written.contains(&id) {
            written.push(id);
        }
    }

    let code = crate::code::notes::open(&cx.home, project)?;
    let changes = match &code {
        Some(c) => work::changed(c)?,
        None => Vec::new(),
    };
    let last_run = state.my_last_run()?;
    let work = super::open::work_folder(cx, &project.name);
    let folder = work.rsplit(['/', '\\']).next().unwrap_or("").to_string();
    let heading = if folder.is_empty() { date_time(now()) } else { format!("{} · {folder}", date_time(now())) };
    let (_, (closed, left)) = update_note(project, plan::FILE, |n, exists, _| {
        if !exists {
            n.props.set_text("type", Kind::Project.as_str());
            n.props.set_text("title", project.name.clone());
        }
        let closed = mine.iter().filter(|s| !a.left.iter().any(|l| l == *s) && plan::close_step(n, s)).count();
        let left = a
            .left
            .iter()
            .filter(|s| plan::add_step(n, s) || plan::steps(n).iter().any(|x| !x.done && x.text == **s))
            .count();
        let mut text = summary.clone();
        if !written.is_empty() {
            let more = written.len().saturating_sub(WRITTEN_SHOWN);
            let list = written.iter().take(WRITTEN_SHOWN).copied().collect::<Vec<_>>().join(", ");
            text.push_str(&format!(
                "\nWritten to memory: {list}{}",
                if more > 0 { format!(" (+{more})") } else { String::new() }
            ));
        }
        if let Some(c) = &code
            && !changes.is_empty()
        {
            text.push_str(&format!("\nChanged: {}", list(c, &changes)));
        }
        if let Some(r) = &last_run {
            text.push_str(&format!("\nTests: {}", testrun::describe(r)));
        }
        plan::add_handoff(n, &heading, &text);
        Ok((closed, left))
    })?;
    state.event("finish", &project.name, summary.lines().next().unwrap_or(""))?;

    let mut out = format!("ok {}: {} closed", project.name, count(closed as i64, "step"));
    if left > 0 {
        out.push_str(&format!(", {} left open", count(left as i64, "step")));
    }
    out.push_str(", handoff saved");
    println!("{out}");
    if let Some(w) = warning {
        println!("{w}");
    }
    if let Some(c) = &code
        && !changes.is_empty()
    {
        println!("changed: {}", list(c, &changes));
        let about = notes_about(c, &Index::open(project)?, &changes)?;
        if !about.is_empty() {
            println!("check the change against:");
            for (l, via) in about.iter().take(NOTES_SHOWN) {
                println!("{} {}: {}{via}", l.id, l.kind.as_str(), one_line(&l.text, 90));
            }
        }
    }
    match &last_run {
        Some(r) => println!("tests: {}", testrun::describe(r)),
        None if !changes.is_empty() => println!("tests: not run in this conversation (pg run -- <test command>)"),
        None => {}
    }
    for line in upkeep::daily(project, &state)? {
        println!("{line}");
    }
    Ok(())
}

fn list(code: &CodeIndex, changes: &[work::Change]) -> String {
    let more = changes.len().saturating_sub(CHANGED_SHOWN);
    let shown: Vec<String> = changes.iter().take(CHANGED_SHOWN).map(|c| c.show(code)).collect();
    format!("{}{}", shown.join(", "), if more > 0 { format!(" (+{more})") } else { String::new() })
}

/// The notes about the changed code, then about the code one call away
/// (its callers and what it calls), each once.
fn notes_about(code: &CodeIndex, index: &Index, changes: &[work::Change]) -> anyhow::Result<Vec<(Linked, String)>> {
    let notes = CodeNotes::load(index)?;
    let mut out: Vec<(Linked, String)> = Vec::new();
    let mut add = |l: &Linked, via: String| {
        if !out.iter().any(|(x, _)| x.id == l.id) {
            out.push((l.clone(), via));
        }
    };
    for c in changes {
        for l in notes.about(&c.path, c.symbol.as_ref().map(|s| s.qualified.as_str())) {
            add(l, String::new());
        }
    }
    for c in changes {
        let Some(s) = &c.symbol else { continue };
        let mut near = code.callers(&s.name)?;
        for call in s.calls.iter().take(20) {
            near.extend(code.named(call)?);
        }
        for n in near {
            for l in notes.about(&n.path, Some(&n.qualified)) {
                add(l, format!(" (via {})", n.qualified));
            }
        }
    }
    Ok(out)
}
