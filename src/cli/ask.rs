//! pg ask / pg answer: the questions that wait on the user, kept in the
//! project note so that no conversation asks one twice or forgets it. The
//! brief lists the open ones.
//!
//! ```text
//! pg ask "Which address goes on the invoices?"   a question for the user
//! pg ask                                         the open questions, numbered
//! pg answer 2 "The registered office"            answered: by number, or words only that question has
//! ```

use super::{Context, NOT_FOUND, USAGE, fail};
use crate::index::update_note;
use crate::note::{Kind, Note};
use crate::project::{Project, files, plan};
use anyhow::Result;

#[derive(clap::Args)]
pub struct AskArgs {
    /// The question for the user; without it, the open questions
    #[arg(allow_hyphen_values = true)]
    question: Vec<String>,
    /// With the answered ones (and their answers)
    #[arg(long)]
    all: bool,
}

#[derive(clap::Args)]
pub struct AnswerArgs {
    /// The question: its number (pg ask lists them) or words only it has
    question: String,
    /// The user's answer
    #[arg(allow_hyphen_values = true, required = true)]
    answer: Vec<String>,
}

pub fn ask(cx: &Context, a: AskArgs) -> Result<()> {
    let project = cx.need_project()?;
    let mut question = a.question.join(" ").trim().to_string();
    if !question.is_empty() {
        let warning = super::mask_all(std::iter::once(&mut question));
        let (_, added) = update_note(project, plan::FILE, |n, exists, _| {
            new_note(n, exists, project);
            Ok(plan::add_question(n, &question))
        })?;
        let number = plan::questions(&read(project)).iter().position(|q| q.answer.is_none() && q.text == question);
        match (added, number) {
            (true, Some(i)) => {
                println!("ok question {} (it shows in the brief until pg answer {} \"...\")", i + 1, i + 1)
            }
            _ => println!("ok already asked: question {}", number.map(|i| i + 1).unwrap_or(0)),
        }
        if let Some(w) = warning {
            println!("{w}");
        }
        return Ok(());
    }
    list(project, a.all);
    Ok(())
}

pub fn answer(cx: &Context, a: AnswerArgs) -> Result<()> {
    let project = cx.need_project()?;
    let mut text = a.answer.join(" ").trim().to_string();
    if text.is_empty() {
        return Err(fail(USAGE, "say the answer: pg answer <number> \"...\""));
    }
    let warning = super::mask_all(std::iter::once(&mut text));
    let (_, result) = update_note(project, plan::FILE, |n, exists, _| {
        new_note(n, exists, project);
        Ok(plan::answer(n, &a.question, &text))
    })?;
    match result {
        Ok(q) => println!("ok answered: {q}"),
        Err(0) => {
            return Err(fail(NOT_FOUND, format!("no open question {:?} (pg ask lists them)", a.question)));
        }
        Err(k) => {
            return Err(fail(USAGE, format!("{k} open questions have {:?}: give its number (pg ask)", a.question)));
        }
    }
    if let Some(w) = warning {
        println!("{w}");
    }
    let left = plan::questions(&read(project)).iter().filter(|q| q.answer.is_none()).count();
    if left > 0 {
        println!("still waiting on the user: {left} (pg ask)");
    }
    Ok(())
}

fn list(project: &Project, all: bool) {
    let qs = plan::questions(&read(project));
    let open = qs.iter().filter(|q| q.answer.is_none()).count();
    if qs.is_empty() || (open == 0 && !all) {
        println!("no open questions{}", if qs.is_empty() { ": pg ask \"...\" keeps one for the user" } else { "" });
        return;
    }
    println!("questions of {}: {open} waiting on the user, {} answered", project.name, qs.len() - open);
    for (i, q) in qs.iter().enumerate() {
        match &q.answer {
            None => println!("? {}. {}", i + 1, q.text),
            Some(ans) if all => println!("✓ {}. {} → {ans}", i + 1, q.text),
            Some(_) => {}
        }
    }
}

fn read(project: &Project) -> Note {
    files::read_text(&project.note_file(plan::FILE)).map(|t| Note::parse(&t)).unwrap_or_default()
}

fn new_note(n: &mut Note, exists: bool, project: &Project) {
    if !exists {
        n.props.set_text("type", Kind::Project.as_str());
        n.props.set_text("title", project.name.clone());
    }
}

impl AskArgs {
    /// Whether it keeps a question (memory then syncs).
    pub fn asks(&self) -> bool {
        !self.question.is_empty()
    }
}
