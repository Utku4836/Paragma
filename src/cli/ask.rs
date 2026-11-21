//! pg ask: the questions that wait on the user, kept in the project note so
//! that no conversation asks one twice or forgets it. The brief lists the
//! open ones. Asking and answering are one command: a number first is the
//! question answered.
//!
//! ```text
//! pg ask "Which address goes on the invoices?"   a question for the user
//! pg ask                                         the open questions, numbered
//! pg ask 2 "The registered office"               question 2 answered
//! pg ask --json '[{"question": "2", "answer": "..."}]'   many answered at once
//! ```
//!
//! `pg answer` (its words, or a number) stays as a hidden name of the same.

use super::{Context, NOT_FOUND, USAGE, fail};
use crate::index::update_note;
use crate::note::{Kind, Note};
use crate::project::{Project, files, plan};
use anyhow::Result;

#[derive(clap::Args)]
#[command(after_help = "Examples:
  pg ask \"Which address goes on the invoices?\"   a question for the user
  pg ask                                         the open questions, numbered
  pg ask 2 \"The registered office\"               question 2 answered
  pg ask --json '[{\"question\": \"2\", \"answer\": \"...\"}]'")]
pub struct AskArgs {
    /// A question for the user; a number and the user's answer answers that question; nothing: the open questions
    #[arg(allow_hyphen_values = true)]
    question: Vec<String>,
    /// With the answered ones (and their answers)
    #[arg(long)]
    all: bool,
    /// Many answers at once, as JSON (or a file of it, - for stdin): [{"question": "2", "answer": "..."}] or {"2": "..."}
    #[arg(long, value_name = "JSON", conflicts_with = "question")]
    json: Option<String>,
}

#[derive(clap::Args)]
pub struct AnswerArgs {
    /// The question: its number (pg ask lists them) or words only it has
    #[arg(required_unless_present = "json")]
    question: Option<String>,
    /// The user's answer
    #[arg(allow_hyphen_values = true, required_unless_present = "json")]
    answer: Vec<String>,
    /// Many answers at once, as JSON (or a file of it, - for stdin): [{"question": "2", "answer": "..."}] or {"2": "..."}
    #[arg(long, value_name = "JSON", conflicts_with_all = ["question", "answer"])]
    json: Option<String>,
}

/// The answers a `--json` gives: (question, answer) pairs, in order.
pub fn answers_of(text: &str) -> Result<Vec<(String, String)>, String> {
    let v: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("give the answers as JSON: [{{\"question\": \"2\", \"answer\": \"...\"}}] ({e})"))?;
    let as_text = |x: &serde_json::Value| match x {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    let pairs: Vec<(String, String)> = match &v {
        serde_json::Value::Object(m) => m.iter().filter_map(|(q, a)| Some((q.clone(), as_text(a)?))).collect(),
        serde_json::Value::Array(items) => {
            items.iter().filter_map(|i| Some((as_text(&i["question"])?, as_text(&i["answer"])?))).collect()
        }
        _ => Vec::new(),
    };
    if pairs.is_empty() {
        return Err("no answers in it: [{\"question\": \"2\", \"answer\": \"...\"}]".into());
    }
    Ok(pairs)
}

pub fn ask(cx: &Context, a: AskArgs) -> Result<()> {
    let project = cx.need_project()?;
    if let Some(json) = &a.json {
        return answer_many(project, json);
    }
    // "2 The registered office": question 2 answered
    if a.question.len() >= 2 && a.question[0].trim_end_matches('.').parse::<usize>().is_ok() {
        let answer = AnswerArgs { question: Some(a.question[0].clone()), answer: a.question[1..].to_vec(), json: None };
        return self::answer(cx, answer);
    }
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
                println!("ok question {} (it shows in the brief until pg ask {} \"<the answer>\")", i + 1, i + 1)
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
    if let Some(json) = &a.json {
        return answer_many(project, json);
    }
    let question = a.question.clone().unwrap_or_default();
    let mut text = a.answer.join(" ").trim().to_string();
    if text.is_empty() {
        return Err(fail(USAGE, "say the answer: pg ask <number> \"...\""));
    }
    let warning = super::mask_all(std::iter::once(&mut text));
    let (_, result) = update_note(project, plan::FILE, |n, exists, _| {
        new_note(n, exists, project);
        Ok(plan::answer(n, &question, &text))
    })?;
    match result {
        Ok(q) => println!("ok answered: {q}"),
        Err(0) => {
            return Err(fail(NOT_FOUND, format!("no open question {question:?} (pg ask lists them)")));
        }
        Err(k) => {
            return Err(fail(USAGE, format!("{k} open questions have {question:?}: give its number (pg ask)")));
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

/// Many answers in one write: each answered or said why not, by number
/// as the questions were before (an answer does not renumber them).
fn answer_many(project: &Project, json: &str) -> Result<()> {
    let text = match json.trim() {
        "-" => {
            let mut t = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut t)?;
            t
        }
        j if !j.starts_with(['[', '{']) && std::path::Path::new(j).is_file() => {
            files::read_text(std::path::Path::new(j))?
        }
        j => j.to_string(),
    };
    let mut pairs = answers_of(&text).map_err(|e| fail(USAGE, e))?;
    let warning = super::mask_all(pairs.iter_mut().map(|(_, a)| a));
    let (_, results) = update_note(project, plan::FILE, |n, exists, _| {
        new_note(n, exists, project);
        Ok(pairs.iter().map(|(q, a)| plan::answer(n, q, a)).collect::<Vec<_>>())
    })?;
    let mut failed = 0;
    for ((q, _), r) in pairs.iter().zip(&results) {
        match r {
            Ok(text) => println!("ok answered: {text}"),
            Err(0) => {
                failed += 1;
                println!("not answered: no open question {q:?} (pg ask lists them)");
            }
            Err(k) => {
                failed += 1;
                println!("not answered: {k} open questions have {q:?}: give its number");
            }
        }
    }
    if let Some(w) = warning {
        println!("{w}");
    }
    let left = plan::questions(&read(project)).iter().filter(|q| q.answer.is_none()).count();
    if left > 0 {
        println!("still waiting on the user: {left} (pg ask)");
    }
    if failed == pairs.len() {
        return Err(fail(NOT_FOUND, "none of them answered (above)"));
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
        !self.question.is_empty() || self.json.is_some()
    }
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/cli/ask.rs"]
mod tests;
