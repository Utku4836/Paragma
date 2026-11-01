//! pg finish: end this conversation's work on the project.
//!
//! ```text
//! pg finish "Client done, refunds next. Tests pass." --left "Move refunds to the client"
//! ```
//!
//! Ticks off the steps this conversation added (not other conversations'
//! steps), adds the steps left for later, and writes this conversation's
//! handoff on top of the project note: the summary, and what it wrote to
//! memory. Each conversation has its own handoff; none overwrites another.

use super::{Context, USAGE, fail};
use crate::index::update_note;
use crate::note::Kind;
use crate::output::{count, date_time, now};
use crate::project::plan;
use crate::record::upkeep;
use crate::state::State;
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

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let project = cx.need_project()?;
    let summary = a.summary.join(" ").trim().to_string();
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
    for line in upkeep::daily(project, &state)? {
        println!("{line}");
    }
    Ok(())
}
