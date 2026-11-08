//! pg steps: the project's steps, numbered as `pg finish --done` takes them.
//!
//! ```text
//! pg steps                     the open steps
//! pg steps --all               the done ones too
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
    /// The done steps too
    #[arg(long)]
    all: bool,
    /// Take steps out of the plan, by number (4,7); the steps after them move up
    #[arg(long, value_delimiter = ',', value_name = "NUMBERS", conflicts_with = "edit")]
    drop: Vec<String>,
    /// Rewrite a step, by number: pg steps --edit 4 "new text"
    #[arg(long, value_name = "NUMBER", requires = "text")]
    edit: Option<usize>,
    /// The step's new text (with --edit)
    #[arg(allow_hyphen_values = true)]
    text: Vec<String>,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let project = cx.need_project()?;
    if !a.text.is_empty() && a.edit.is_none() {
        return Err(fail(USAGE, "a text is for --edit: pg steps --edit <number> \"new text\""));
    }
    if !a.drop.is_empty() || a.edit.is_some() {
        let text = a.text.join(" ");
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
            // the highest first: the numbers asked for stay the ones shown
            let mut drop = Vec::new();
            for d in numbers_apart(&a.drop) {
                match d.trim().trim_end_matches('.').parse::<usize>() {
                    Ok(n) => drop.push(n),
                    Err(_) => return Err(fail(USAGE, format!("--drop takes step numbers (pg steps), not {d:?}"))),
                }
            }
            drop.sort_unstable_by(|x, y| y.cmp(x));
            drop.dedup();
            let mut dropped: i64 = 0;
            for number in drop {
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
    let open = steps.iter().filter(|s| !s.done).count();
    if steps.is_empty() {
        println!("no steps yet: pg open --step \"...\"");
        return Ok(());
    }
    println!("steps of {}: {open} open, {} done", project.name, steps.len() - open);
    for (i, s) in steps.iter().enumerate().filter(|(_, s)| a.all || !s.done) {
        println!("- [{}] {}. {}", if s.done { "x" } else { " " }, i + 1, s.text);
    }
    if open > 0 {
        println!("(done: pg finish \"...\" --done <numbers>)");
    }
    Ok(())
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
        !self.drop.is_empty() || self.edit.is_some()
    }
}
