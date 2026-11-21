//! pg steps: the project's steps, numbered as `pg finish --done` takes them.
//!
//! ```text
//! pg steps                     the open steps (and how many are deferred)
//! pg steps --all               the done and deferred ones too
//! pg steps --defer 4,7         left for later on purpose; --resume 4 brings one back
//! pg steps --reopen 4          open a step closed by mistake again
//! pg steps --drop 4,7          take steps out of the plan
//! pg steps --edit 4 "text"     rewrite a step
//! ```

use super::{Context, NOT_FOUND, USAGE, fail};
use crate::index::update_note;
use crate::note::Kind;
use crate::output::count;
use crate::project::{files, plan};
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// The done and deferred steps too
    #[arg(long)]
    all: bool,
    /// Leave steps for later on purpose, by number (4,7): out of the brief's open steps until --resume
    #[arg(long, value_delimiter = ',', value_name = "NUMBERS")]
    defer: Vec<String>,
    /// Bring deferred steps back to the work now, by number
    #[arg(long, value_delimiter = ',', value_name = "NUMBERS")]
    resume: Vec<String>,
    /// Open done steps again (closed by mistake), by number
    #[arg(long, value_delimiter = ',', value_name = "NUMBERS")]
    reopen: Vec<String>,
    /// Take steps out of the plan, by number (4,7); every other step keeps its number
    #[arg(long, value_delimiter = ',', value_name = "NUMBERS", conflicts_with = "edit")]
    drop: Vec<String>,
    /// Rewrite a step, by number: pg steps --edit 4 "new text"
    #[arg(long, value_name = "NUMBER", requires = "text")]
    edit: Option<usize>,
    /// The step's new text (with --edit)
    #[arg(allow_hyphen_values = true)]
    text: Vec<String>,
    /// Take a step, by number: the other conversations here see it taken until this one hands off or goes quiet
    #[arg(long, value_name = "NUMBER")]
    take: Option<usize>,
    /// With --take: take it even when another conversation has it
    #[arg(long, requires = "take")]
    force: bool,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let project = cx.need_project()?;
    if !a.text.is_empty() && a.edit.is_none() {
        return Err(fail(USAGE, "a text is for --edit: pg steps --edit <number> \"new text\""));
    }
    let state = crate::state::State::open(project)?;
    if let Some(number) = a.take {
        take(project, &state, number, a.force, &cx.cwd.to_string_lossy())?;
    }
    if a.changes() {
        let text = a.text.join(" ");
        let defer = numbers(&a.defer, "--defer")?;
        let resume = numbers(&a.resume, "--resume")?;
        let reopen = numbers(&a.reopen, "--reopen")?;
        let mut drop = numbers(&a.drop, "--drop")?;
        let (_, said) = update_note(project, plan::FILE, |n, exists, _| {
            if !exists {
                n.props.set_text("type", Kind::Project.as_str());
                n.props.set_text("title", project.name.clone());
            }
            let mut said = Vec::new();
            if let Some(number) = a.edit {
                match plan::edit_number(n, number, &text) {
                    Some(_) => said.push(format!("ok step {number} rewritten")),
                    None => return Err(fail(NOT_FOUND, format!("no step {number} (pg steps --all lists them)"))),
                }
            }
            for (list, later, what) in [(&defer, true, "deferred"), (&resume, false, "back in the work")] {
                if list.is_empty() {
                    continue;
                }
                let mut changed: i64 = 0;
                for &number in list {
                    match plan::defer_number(n, number, later) {
                        Some(_) => changed += 1,
                        None => said.push(format!("no step {number} that is not done")),
                    }
                }
                said.insert(0, format!("ok {} {what}", count(changed, "step")));
            }
            if !reopen.is_empty() {
                let mut opened: i64 = 0;
                for &number in &reopen {
                    match plan::reopen_number(n, number) {
                        Some(_) => opened += 1,
                        None => said.push(format!("no done step {number}")),
                    }
                }
                said.insert(0, format!("ok {} open again", count(opened, "step")));
            }
            drop.sort_unstable();
            drop.dedup();
            let mut dropped: i64 = 0;
            for &number in &drop {
                match plan::drop_number(n, number) {
                    Some(_) => dropped += 1,
                    None => said.push(format!("no step {number}")),
                }
            }
            if !a.drop.is_empty() {
                said.insert(0, format!("ok {} taken out", count(dropped, "step")));
            }
            Ok(said)
        })?;
        for l in said {
            println!("{l}");
        }
    }
    let note =
        files::read_text(&project.note_file(plan::FILE)).map(|t| crate::note::Note::parse(&t)).unwrap_or_default();
    let steps = plan::steps(&note);
    if steps.is_empty() {
        println!("no steps yet: pg open --step \"...\"");
        return Ok(());
    }
    let open = steps.iter().filter(|s| s.open()).count();
    let deferred = steps.iter().filter(|s| s.deferred).count();
    let facts = super::perceive::facts(project, &state, false)?;
    let claims = crate::kernel::experts::claims(&facts);
    println!(
        "steps of {}: {open} open, {} done{}",
        project.name,
        steps.iter().filter(|s| s.done).count(),
        if deferred > 0 { format!(", {deferred} deferred") } else { String::new() }
    );
    for (i, s) in steps.iter().enumerate().filter(|(_, s)| !s.dropped && (a.all || s.open())) {
        let mark = if s.done {
            "x"
        } else if s.deferred {
            "~"
        } else {
            " "
        };
        let taken = if s.open() { crate::kernel::experts::claim_mark(&facts, &claims, i + 1) } else { String::new() };
        println!("- [{mark}] {}. {}{}{taken}", i + 1, s.text, if s.deferred { " (deferred)" } else { "" });
    }
    if deferred > 0 && !a.all {
        println!("(deferred ones: pg steps --all; back to the work: pg steps --resume <number>)");
    }
    if open > 0 {
        println!("(done: pg finish \"...\" --done <numbers>)");
    }
    for line in super::perceive::news(project, &state)? {
        println!("{line}");
    }
    Ok(())
}

/// Takes a step for this conversation, unless another one working now has
/// it (then only with --force).
fn take(
    project: &crate::project::Project,
    state: &crate::state::State,
    number: usize,
    force: bool,
    folder: &str,
) -> Result<()> {
    let note =
        files::read_text(&project.note_file(plan::FILE)).map(|t| crate::note::Note::parse(&t)).unwrap_or_default();
    let steps = plan::steps(&note);
    let Some(step) = number.checked_sub(1).and_then(|i| steps.get(i)).filter(|s| s.open()) else {
        return Err(fail(NOT_FOUND, format!("no open step {number} (pg steps lists them)")));
    };
    let facts = super::perceive::facts(project, state, false)?;
    let claims = crate::kernel::experts::claims(&facts);
    if claims.get(&number).is_some_and(|c| c.conversation != facts.me) && !force {
        // " (taken by shop, 5 min ago)" -> "taken by shop, 5 min ago"
        let mark = crate::kernel::experts::claim_mark(&facts, &claims, number);
        let mark = mark.trim().trim_start_matches('(').trim_end_matches(')');
        return Err(fail(
            USAGE,
            format!("step {number} is {mark}: leave it, or take it over with pg steps --take {number} --force"),
        ));
    }
    // the others see who took it: where this conversation works
    state.touch_session(folder)?;
    state.record(crate::state::journal::Did::Take { step: number })?;
    println!(
        "ok step {number} is yours ({}): the other conversations here see it taken until you hand off or go quiet for an hour",
        crate::output::one_line(&step.text, 60)
    );
    Ok(())
}

/// Step numbers as given (4,7 or "4 7"); a usage error for anything else.
fn numbers(values: &[String], flag: &str) -> Result<Vec<usize>> {
    numbers_apart(values)
        .iter()
        .map(|d| {
            d.trim()
                .trim_end_matches('.')
                .parse::<usize>()
                .map_err(|_| fail(USAGE, format!("{flag} takes step numbers (pg steps), not {d:?}")))
        })
        .collect()
}

/// "1 3" as 1 and 3: PowerShell hands `--done 1,3` on as "1 3" through
/// a script (npm's pg.ps1 before Paragma rewrites it).
pub fn numbers_apart(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for v in values {
        let words: Vec<&str> = v.split_whitespace().collect();
        if words.len() > 1 && words.iter().all(|w| w.trim_end_matches('.').parse::<usize>().is_ok()) {
            out.extend(words.iter().map(|w| w.to_string()));
        } else {
            out.push(v.clone());
        }
    }
    out
}

impl Args {
    /// Whether it changes the plan (memory then syncs).
    pub fn changes(&self) -> bool {
        self.take.is_some()
            || !self.drop.is_empty()
            || self.edit.is_some()
            || !self.defer.is_empty()
            || !self.resume.is_empty()
            || !self.reopen.is_empty()
    }
}
