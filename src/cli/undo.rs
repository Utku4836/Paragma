//! pg undo: what pg did by itself, inside memory, listed, and one taken
//! back. pg acts on its own only in small steps that undo cleanly: a
//! handoff written for a conversation that stopped without one, a draft
//! record of an error a test run fixed. Each is in the event log (`auto`),
//! and undoing one is too (`undone`).
//!
//! ```text
//! pg undo            what pg did by itself lately, with numbers
//! pg undo 812        takes that one back
//! pg undo last       the latest one
//! ```

use super::{Context, NOT_FOUND, USAGE, fail};
use crate::index::update_note;
use crate::output::{ago, now};
use crate::project::{Project, plan};
use crate::state::State;
use crate::state::journal::{Did, Entry};
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// The number of what to take back (pg undo lists them), or last
    which: Option<String>,
}

impl Args {
    pub fn changes(&self) -> bool {
        self.which.is_some()
    }
}

/// How far back pg undo looks.
const DAYS: i64 = 30;

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let project = cx.need_project()?;
    let state = State::open(project)?;
    let open = undoable(&state)?;
    let Some(which) = a.which else {
        if open.is_empty() {
            println!("pg did nothing by itself here lately (nothing to undo)");
            return Ok(());
        }
        println!("what pg did by itself ({}), newest first:", open.len());
        for e in open.iter().rev().take(20) {
            println!("{}. {} ({})", e.seq, describe(e), ago(e.at, now()));
        }
        println!("take one back: pg undo <number>");
        return Ok(());
    };
    let entry = if which.eq_ignore_ascii_case("last") {
        open.last()
    } else {
        let n: i64 = which
            .trim_start_matches('#')
            .parse()
            .map_err(|_| fail(USAGE, "give its number (pg undo lists them) or last"))?;
        open.iter().find(|e| e.seq == n)
    };
    let Some(entry) = entry else {
        return Err(fail(NOT_FOUND, format!("nothing pg did by itself is numbered {which} (pg undo lists them)")));
    };
    let said = take_back(project, &state, entry)?;
    state.record(Did::Undone { of: entry.seq })?;
    println!("ok undone {}: {said}", entry.seq);
    Ok(())
}

/// pg's own actions of the last days that were not taken back, oldest first.
pub fn undoable(state: &State) -> Result<Vec<Entry>> {
    let all = state.journal(now() - DAYS * 86_400)?;
    let undone: Vec<i64> =
        all.iter().filter_map(|e| if let Did::Undone { of } = e.did { Some(of) } else { None }).collect();
    Ok(all.into_iter().filter(|e| matches!(e.did, Did::Auto { .. }) && !undone.contains(&e.seq)).collect())
}

/// One line on what pg did.
pub fn describe(e: &Entry) -> String {
    let Did::Auto { what, target, detail } = &e.did else { return String::new() };
    match what.as_str() {
        "handoff" => format!("wrote a handoff for a conversation that stopped without one ({detail})"),
        "draft" => format!("wrote {target}, a draft record of an error a test run fixed"),
        other => format!("{other} {target}"),
    }
}

/// Takes one action back; what it did.
fn take_back(project: &Project, state: &State, e: &Entry) -> Result<String> {
    let Did::Auto { what, target, detail } = &e.did else { return Ok(String::new()) };
    match what.as_str() {
        "handoff" => {
            let (_, found) = update_note(project, plan::FILE, |n, _, _| {
                let before = plan::handoffs(n).len();
                plan::remove_handoff(n, detail);
                Ok(plan::handoffs(n).len() < before)
            })?;
            // that conversation may write its own again
            if state.get(&format!("handoff:{target}")).as_deref() == Some(detail.as_str()) {
                state.set(&format!("handoff:{target}"), "")?;
            }
            Ok(if found { format!("the handoff {detail} is out") } else { "that handoff was already gone".into() })
        }
        "draft" => {
            let path = project.dir.join(detail);
            let still_draft = crate::project::files::read_text(&path)
                .map(|t| crate::note::Note::parse(&t).props.text("status") == Some("draft"))
                .unwrap_or(false);
            if !still_draft {
                return Err(fail(
                    USAGE,
                    format!("{target} is no longer a draft (confirmed or changed): pg rm {target} takes it out"),
                ));
            }
            super::rm::take_out(project, detail, false)?;
            Ok(format!("{target} moved to .archive (pg mv brings it back)"))
        }
        other => Err(fail(USAGE, format!("this pg does not know how to take back {other:?}"))),
    }
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/cli/undo.rs"]
mod tests;
