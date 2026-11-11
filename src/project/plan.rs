//! The project note (`project.md`): what the work is for, its steps and
//! what each conversation handed over to the next.
//!
//! ```text
//! ---
//! id: p1
//! type: project
//! title: shop
//! ---
//! ## Goal
//! Refunds through the new provider.
//!
//! ## Steps
//! - [x] Add the provider client
//! - [ ] Move refunds to it
//!
//! ## Handoffs
//! ### 2026-11-01 14:10Z · shop-api
//! Client done, refunds next. Tests pass.
//! ```

use crate::note::Note;

/// The project note's path in the project.
pub const FILE: &str = "project.md";

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub done: bool,
    /// Left for later on purpose (`- [~]`): not done, not in the work now.
    pub deferred: bool,
    pub text: String,
}

impl Step {
    /// To be done now: neither done nor deferred.
    pub fn open(&self) -> bool {
        !self.done && !self.deferred
    }
}

/// A question that waits on the user: `- [ ] question`, answered
/// `- [x] question → answer`.
#[derive(Clone, Debug, PartialEq)]
pub struct Question {
    pub text: String,
    pub answer: Option<String>,
}

/// "1. Products" as a step's text: its own number would show twice next
/// to the one pg gives it.
fn without_number(text: &str) -> &str {
    let t = text.trim();
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 3 {
        return t;
    }
    let rest = &t[digits..];
    match rest.strip_prefix(['.', ')']) {
        Some(r) if r.starts_with(' ') => r.trim_start(),
        _ => t,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Handoff {
    /// "2026-11-01 14:10Z · shop-api"
    pub heading: String,
    pub text: String,
}

pub fn goal(note: &Note) -> String {
    note.section("Goal").unwrap_or_default()
}

pub fn set_goal(note: &mut Note, goal: &str) {
    note.set_section("Goal", goal);
}

pub fn steps(note: &Note) -> Vec<Step> {
    let section = note.section("Steps").unwrap_or_default();
    section
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let (done, deferred, text) = if let Some(t) = l.strip_prefix("- [ ] ") {
                (false, false, t)
            } else if let Some(t) = l.strip_prefix("- [~] ") {
                (false, true, t)
            } else {
                (true, false, l.strip_prefix("- [x] ").or_else(|| l.strip_prefix("- [X] "))?)
            };
            Some(Step { done, deferred, text: without_number(text).to_string() })
        })
        .collect()
}

fn set_steps(note: &mut Note, steps: &[Step]) {
    let lines: Vec<String> = steps
        .iter()
        .map(|s| {
            format!(
                "- [{}] {}",
                if s.done {
                    "x"
                } else if s.deferred {
                    "~"
                } else {
                    " "
                },
                s.text
            )
        })
        .collect();
    note.set_section("Steps", &lines.join("\n"));
}

/// Defers the step with this number (`later`), or brings it back to the
/// work now: its text, or None when there is no such step not done.
pub fn defer_number(note: &mut Note, n: usize, later: bool) -> Option<String> {
    let mut steps = steps(note);
    let s = steps.get_mut(n.checked_sub(1)?).filter(|s| !s.done)?;
    s.deferred = later;
    let text = s.text.clone();
    set_steps(note, &steps);
    Some(text)
}

pub fn questions(note: &Note) -> Vec<Question> {
    let section = note.section("Questions").unwrap_or_default();
    section
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            if let Some(t) = l.strip_prefix("- [ ] ") {
                return Some(Question { text: t.trim().to_string(), answer: None });
            }
            let t = l.strip_prefix("- [x] ").or_else(|| l.strip_prefix("- [X] "))?;
            let (q, a) = t.rsplit_once(" → ").unwrap_or((t, ""));
            Some(Question { text: q.trim().to_string(), answer: Some(a.trim().to_string()) })
        })
        .collect()
}

fn set_questions(note: &mut Note, qs: &[Question]) {
    let lines: Vec<String> = qs
        .iter()
        .map(|q| match &q.answer {
            None => format!("- [ ] {}", q.text),
            Some(a) => format!("- [x] {} → {a}", q.text),
        })
        .collect();
    note.set_section("Questions", &lines.join("\n"));
}

/// Adds a question that waits on the user; false when it is already open.
pub fn add_question(note: &mut Note, text: &str) -> bool {
    let text = text.trim();
    let mut qs = questions(note);
    if text.is_empty() || qs.iter().any(|q| q.answer.is_none() && q.text == text) {
        return false;
    }
    qs.push(Question { text: text.to_string(), answer: None });
    set_questions(note, &qs);
    true
}

/// Answers the open question with this number (from 1, answered ones
/// counted) or the one open question whose text holds `which`: its text;
/// Err with how many open questions `which` fits when not exactly one.
pub fn answer(note: &mut Note, which: &str, answer: &str) -> Result<String, usize> {
    let mut qs = questions(note);
    let which = which.trim();
    let hits: Vec<usize> = match which.trim_end_matches('.').parse::<usize>() {
        Ok(n) => (n >= 1 && n <= qs.len() && qs[n - 1].answer.is_none()).then_some(n - 1).into_iter().collect(),
        Err(_) => {
            let part = which.to_lowercase();
            (0..qs.len())
                .filter(|&i| qs[i].answer.is_none() && !part.is_empty() && qs[i].text.to_lowercase().contains(&part))
                .collect()
        }
    };
    let [i] = hits[..] else { return Err(hits.len()) };
    qs[i].answer = Some(answer.trim().replace('\n', " "));
    let text = qs[i].text.clone();
    set_questions(note, &qs);
    Ok(text)
}

/// Adds an open step; false when the same step is already open.
pub fn add_step(note: &mut Note, text: &str) -> bool {
    let text = without_number(text);
    let mut steps = steps(note);
    if text.is_empty() || steps.iter().any(|s| !s.done && s.text == text) {
        return false;
    }
    steps.push(Step { done: false, deferred: false, text: text.to_string() });
    set_steps(note, &steps);
    true
}

/// Ticks off an open step; false when there is no such open step.
pub fn close_step(note: &mut Note, text: &str) -> bool {
    let mut steps = steps(note);
    let Some(s) = steps.iter_mut().find(|s| !s.done && s.text == text.trim()) else {
        return false;
    };
    s.done = true;
    set_steps(note, &steps);
    true
}

/// Ticks off the step with this number (from 1, in the order of the note,
/// done steps counted): its text, or None when there is no such open step.
pub fn close_number(note: &mut Note, n: usize) -> Option<String> {
    let mut steps = steps(note);
    let s = steps.get_mut(n.checked_sub(1)?).filter(|s| !s.done)?;
    s.done = true;
    let text = s.text.clone();
    set_steps(note, &steps);
    Some(text)
}

/// Ticks off the one open step whose text holds `part` (in any case): its
/// text; Err with how many open steps hold it when not exactly one does.
pub fn close_part(note: &mut Note, part: &str) -> Result<String, usize> {
    let part = part.trim().to_lowercase();
    let mut steps = steps(note);
    let hits: Vec<usize> = (0..steps.len())
        .filter(|&i| !steps[i].done && !part.is_empty() && steps[i].text.to_lowercase().contains(&part))
        .collect();
    let [i] = hits[..] else { return Err(hits.len()) };
    steps[i].done = true;
    let text = steps[i].text.clone();
    set_steps(note, &steps);
    Ok(text)
}

/// Takes the step with this number (from 1, done steps counted) out of
/// the plan: its text, or None when there is no such step. The steps
/// after it move up one number.
pub fn drop_number(note: &mut Note, n: usize) -> Option<String> {
    let mut steps = steps(note);
    let i = n.checked_sub(1).filter(|&i| i < steps.len())?;
    let s = steps.remove(i);
    set_steps(note, &steps);
    Some(s.text)
}

/// Rewrites the step with this number: its old text, or None when there
/// is no such step.
pub fn edit_number(note: &mut Note, n: usize, text: &str) -> Option<String> {
    let mut steps = steps(note);
    let s = steps.get_mut(n.checked_sub(1)?)?;
    let old = std::mem::replace(&mut s.text, text.trim().to_string());
    set_steps(note, &steps);
    Some(old)
}

/// Handoffs, newest first.
pub fn handoffs(note: &Note) -> Vec<Handoff> {
    let section = note.section("Handoffs").unwrap_or_default();
    let mut out: Vec<Handoff> = Vec::new();
    for line in section.lines() {
        if let Some(h) = line.strip_prefix("### ") {
            out.push(Handoff { heading: h.trim().to_string(), text: String::new() });
        } else if let Some(last) = out.last_mut() {
            last.text.push_str(line);
            last.text.push('\n');
        }
    }
    for h in &mut out {
        h.text = h.text.trim().to_string();
    }
    out
}

/// Adds a handoff on top (the newest is read first), in place of the one
/// headed `replaces` (the same conversation finishing again).
pub fn add_handoff(note: &mut Note, heading: &str, text: &str, replaces: Option<&str>) {
    let mut all = handoffs(note);
    all.retain(|h| Some(h.heading.as_str()) != replaces);
    all.insert(0, Handoff { heading: heading.to_string(), text: text.trim().to_string() });
    let section: Vec<String> = all.iter().map(|h| format!("### {}\n{}", h.heading, h.text)).collect();
    note.set_section("Handoffs", &section.join("\n\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_open_and_close() {
        let mut n = Note::default();
        set_goal(&mut n, "Refunds through the new provider.");
        assert!(add_step(&mut n, "Add the client"));
        assert!(add_step(&mut n, "Move refunds"));
        assert!(!add_step(&mut n, "Move refunds"));
        assert_eq!(close_number(&mut n, 1).as_deref(), Some("Add the client"));
        assert_eq!(close_number(&mut n, 1), None);
        assert_eq!(close_number(&mut n, 9), None);
        assert!(!close_step(&mut n, "Add the client"));
        assert_eq!(
            n.body,
            "## Goal\nRefunds through the new provider.\n\n## Steps\n- [x] Add the client\n- [ ] Move refunds\n"
        );
        assert_eq!(goal(&n), "Refunds through the new provider.");
        assert_eq!(steps(&n).iter().filter(|s| !s.done).count(), 1);
    }

    #[test]
    fn steps_by_part_dropped_and_rewritten() {
        let mut n = Note::default();
        for s in ["Add the client", "Move refunds to the client", "Remove the old provider", "x"] {
            add_step(&mut n, s);
        }
        // a part two open steps hold closes none
        assert_eq!(close_part(&mut n, "the client"), Err(2));
        assert_eq!(close_part(&mut n, "nothing like it"), Err(0));
        assert_eq!(close_part(&mut n, "MOVE REFUNDS").as_deref(), Ok("Move refunds to the client"));
        assert_eq!(close_part(&mut n, "the client").as_deref(), Ok("Add the client"));
        assert_eq!(drop_number(&mut n, 4).as_deref(), Some("x"));
        assert_eq!(drop_number(&mut n, 4), None);
        assert_eq!(
            edit_number(&mut n, 3, "Remove the old provider and its keys").as_deref(),
            Some("Remove the old provider")
        );
        let texts: Vec<String> = steps(&n).into_iter().map(|s| s.text).collect();
        assert_eq!(texts, ["Add the client", "Move refunds to the client", "Remove the old provider and its keys"]);
    }

    #[test]
    fn the_newest_handoff_comes_first() {
        let mut n = Note::default();
        add_handoff(&mut n, "2026-10-31 09:00Z · web", "Login page done.", None);
        add_handoff(&mut n, "2026-11-01 14:00Z · api", "Client half done.", None);
        add_handoff(&mut n, "2026-11-01 14:10Z · api", "Client done.\nTests pass.", Some("2026-11-01 14:00Z · api"));
        let h = handoffs(&n);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].heading, "2026-11-01 14:10Z · api");
        assert_eq!(h[0].text, "Client done.\nTests pass.");
        assert_eq!(h[1].text, "Login page done.");
    }
}
