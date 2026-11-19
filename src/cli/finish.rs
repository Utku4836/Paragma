//! pg finish: end this conversation's work on the project.
//!
//! ```text
//! pg finish "Client done, refunds next. Tests pass." --done 1,2 --left "Move refunds to the client"
//! ```
//!
//! Ticks off the steps it is told are done (by their number in the brief),
//! adds the steps left for later, and writes this conversation's handoff on
//! top of the project note: the summary, and what it wrote to memory, the
//! code it changed and how its last test run went. Each conversation has
//! its own handoff: finishing again replaces it, and no other one.
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

    /// The steps done: their numbers (1,3; pg steps lists them), words only that step has, or all
    #[arg(long, value_delimiter = ',', value_name = "STEPS")]
    done: Vec<String>,

    /// A step left for later (repeat for more)
    #[arg(long = "left", value_name = "STEP", allow_hyphen_values = true)]
    left: Vec<String>,
}

/// The step numbers after `--done`'s own value, before the next flag: given
/// apart (`--done 3 4 5`), the shell makes them separate words.
pub fn numbers_after_done(args: &[String]) -> Vec<String> {
    let Some(at) = args.iter().position(|a| a == "--done") else { return Vec::new() };
    args.iter()
        .skip(at + 2)
        .take_while(|a| !a.starts_with('-') && a.trim_end_matches([',', '.']).parse::<usize>().is_ok())
        .cloned()
        .collect()
}

/// Ids of written notes listed in a handoff.
const WRITTEN_SHOWN: usize = 10;
/// Changed symbols named in a handoff.
const CHANGED_SHOWN: usize = 8;
/// Notes about the changed code listed.
const NOTES_SHOWN: usize = 8;

pub fn run(cx: &Context, mut a: Args) -> Result<()> {
    let project = cx.need_project()?;
    // `--done 3 4 5`: the shell gives 4 and 5 as words of the summary
    for n in numbers_after_done(&std::env::args().collect::<Vec<_>>()) {
        if let Some(i) = a.summary.iter().rposition(|w| *w == n) {
            a.summary.remove(i);
            a.done.push(n.trim_end_matches([',', '.']).to_string());
        }
    }
    let mut summary = a.summary.join(" ").trim().to_string();
    let warning = super::mask_all(std::iter::once(&mut summary).chain(a.left.iter_mut()));
    if summary.is_empty() {
        return Err(fail(USAGE, "say what was done: pg finish \"what was done; what is next\""));
    }
    let state = State::open(project)?;
    let last_finish = state.my_last("finish").unwrap_or(0);
    // the steps this conversation took: those it leaves open are named in
    // its handoff as worked on, and asked about (cli/act.rs)
    let taken = super::act::taken_open(project, &state, last_finish)?;
    crate::code::git::changed_ahead(crate::state::links::Links::load(&cx.home).folders_of(&project.name));
    let code = crate::code::notes::open(&cx.home, project)?;
    let changes = match &code {
        Some(c) => work::changed(c)?,
        None => Vec::new(),
    };
    let last_run = state.my_last_run()?;
    // a change that left callers as they were: in the handoff too
    let ripples = code.as_ref().map(|c| super::ripple::warnings(project, c, None, true)).unwrap_or_default();
    let work = super::open::work_folder(cx, &project.name);
    let folder = work.rsplit(['/', '\\']).next().unwrap_or("").to_string();
    let heading = if folder.is_empty() { date_time(now()) } else { format!("{} · {folder}", date_time(now())) };
    // this conversation's earlier handoff, replaced by this one
    let mine = format!("handoff:{}", crate::state::conversation::key());
    let earlier = state.get(&mine);
    // a handoff that replaces this conversation's last one tells all since the one before
    let events = state.my_events(usize::from(earlier.is_some()))?;
    let mut written: Vec<&str> = Vec::new();
    for e in events.iter().filter(|e| e.action == "write") {
        let id = e.detail.split(' ').next().unwrap_or("");
        if !id.is_empty() && !written.contains(&id) {
            written.push(id);
        }
    }
    let (_, (closed, left, open, missed, named, worked)) = update_note(project, plan::FILE, |n, exists, _| {
        if !exists {
            n.props.set_text("type", Kind::Project.as_str());
            n.props.set_text("title", project.name.clone());
        }
        let mut closed = 0;
        let done_before: Vec<bool> = plan::steps(n).iter().map(|s| s.done).collect();
        // what --done named that closed nothing, and why
        let mut missed = Vec::new();
        for d in super::steps::numbers_apart(&a.done).iter().map(|d| d.trim()).filter(|d| !d.is_empty()) {
            if d.eq_ignore_ascii_case("all") {
                let open: Vec<String> = plan::steps(n).into_iter().filter(|s| s.open()).map(|s| s.text).collect();
                closed += open.iter().filter(|s| plan::close_step(n, s)).count();
            } else if let Ok(number) = d.trim_end_matches('.').parse::<usize>() {
                match plan::close_number(n, number) {
                    Some(_) => closed += 1,
                    None => missed.push(format!("{number}: no open step has that number")),
                }
            } else if plan::close_step(n, d) {
                closed += 1;
            } else {
                match plan::close_part(n, d) {
                    Ok(_) => closed += 1,
                    Err(0) => missed.push(format!("{:?}: no open step has those words", one_line(d, 40))),
                    Err(k) => missed.push(format!("{:?}: {k} open steps have those words", one_line(d, 40))),
                }
            }
        }
        let left = a
            .left
            .iter()
            .filter(|s| plan::add_step(n, s) || plan::steps(n).iter().any(|x| !x.done && x.text == **s))
            .count();
        let mut text = summary.clone();
        // the steps it took and leaves open: the next one knows they were begun
        let worked: Vec<String> = taken
            .iter()
            .filter(|(k, _)| plan::steps(n).get(k - 1).is_some_and(|s| s.open()))
            .map(|(k, t)| format!("{k}. {}", one_line(t, 60)))
            .collect();
        if !worked.is_empty() {
            text.push_str(&format!("\nWorked on, still open: {}", worked.join("; ")));
        }
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
        if !ripples.is_empty() {
            text.push_str(&format!("\nLeft as they were after a change: {}", ripples.join("; ")));
        }
        if let Some(r) = &last_run {
            text.push_str(&format!("\nTests: {}", testrun::describe(r)));
        }
        plan::add_handoff(n, &heading, &text, earlier.as_deref());
        let open: Vec<String> = plan::steps(n)
            .iter()
            .enumerate()
            .filter(|(_, s)| s.open())
            .map(|(i, s)| format!("{}. {}", i + 1, one_line(&s.text, 60)))
            .collect();
        // what was closed, by number and text: a wrong one is seen at once
        let named: Vec<String> = plan::steps(n)
            .iter()
            .enumerate()
            .filter(|(i, s)| s.done && !done_before.get(*i).copied().unwrap_or(false))
            .map(|(i, s)| format!("{}. {}", i + 1, one_line(&s.text, 70)))
            .collect();
        Ok((closed, left, open, missed, named, worked))
    })?;
    state.set(&mine, &heading)?;
    // fixes shown whose tests still fail did not help (state/outcome.rs)
    state.outcomes_at_handoff()?;
    state.record(crate::state::journal::Did::Finish { summary: summary.lines().next().unwrap_or("").to_string() })?;

    let mut out = format!("ok {}: {} closed", project.name, count(closed as i64, "step"));
    if left > 0 {
        out.push_str(&format!(", {} left open", count(left as i64, "step")));
    }
    out.push_str(if earlier.is_some() { ", handoff replaced" } else { ", handoff saved" });
    println!("{out}");
    if !named.is_empty() {
        println!("closed: {} (a wrong one: pg steps --reopen <number>)", named.join("; "));
    }
    if !worked.is_empty() {
        let numbers: Vec<&str> = worked.iter().filter_map(|w| w.split('.').next()).collect();
        println!(
            "you took these and they are still open: {} (the handoff says you worked on them); done? pg finish again with --done {}",
            worked.join("; "),
            numbers.join(",")
        );
    }
    for m in &missed {
        println!("not closed: {m} (pg steps lists them with their numbers)");
    }
    if !open.is_empty() {
        let more = open.len().saturating_sub(5);
        println!(
            "open steps: {}{} (a done one: pg finish again with --done <number>; it replaces this handoff)",
            open.iter().take(5).cloned().collect::<Vec<_>>().join("; "),
            if more > 0 { format!("; +{more}: pg steps") } else { String::new() }
        );
    }
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
    if !ripples.is_empty() {
        println!("not updated after a change (finish it, or say so in the handoff):");
        for r in &ripples {
            println!("{r}");
        }
    }
    match &last_run {
        Some(r) => println!("tests: {}", testrun::describe(r)),
        None if !changes.is_empty() => println!("tests: not run in this conversation (pg run -- <test command>)"),
        None => {}
    }
    // errors this conversation fixed that memory still has no fix for
    let index = Index::open(project)?;
    for line in state.my_targets_since("fixed", last_finish)? {
        if super::run::known(&index, project, &line)?.is_none() {
            println!(
                "fixed here, not in memory: pg write --error {:?} --cause \"...\" --fix \"...\"",
                one_line(&line, 120)
            );
        }
    }
    for line in upkeep::daily(project, &state)? {
        println!("{line}");
    }
    // what the last sleep found to tidy, once a conversation; then the
    // next sleep looks over the memory again, in the background
    for line in super::perceive::news(&state)? {
        println!("{line}");
    }
    let facts = super::perceive::facts(project, &state, false)?;
    let hints = crate::kernel::finish_hints(&facts, 120);
    for h in &hints {
        println!("{}", h.text);
    }
    state.say(&hints.iter().map(|h| h.key.clone()).collect::<Vec<_>>())?;
    super::sleep::start(cx, project);
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

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/cli/finish.rs"]
mod tests;
