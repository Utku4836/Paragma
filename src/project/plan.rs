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
    pub text: String,
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
            let (done, text) = if let Some(t) = l.strip_prefix("- [ ] ") {
                (false, t)
            } else {
                (true, l.strip_prefix("- [x] ").or_else(|| l.strip_prefix("- [X] "))?)
            };
            Some(Step { done, text: text.trim().to_string() })
        })
        .collect()
}

fn set_steps(note: &mut Note, steps: &[Step]) {
    let lines: Vec<String> =
        steps.iter().map(|s| format!("- [{}] {}", if s.done { "x" } else { " " }, s.text)).collect();
    note.set_section("Steps", &lines.join("\n"));
}

/// Adds an open step; false when the same step is already open.
pub fn add_step(note: &mut Note, text: &str) -> bool {
    let text = text.trim();
    let mut steps = steps(note);
    if text.is_empty() || steps.iter().any(|s| !s.done && s.text == text) {
        return false;
    }
    steps.push(Step { done: false, text: text.to_string() });
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

/// Adds a handoff on top: the newest is read first.
pub fn add_handoff(note: &mut Note, heading: &str, text: &str) {
    let old = note.section("Handoffs").unwrap_or_default();
    let new = format!("### {heading}\n{}", text.trim());
    note.set_section("Handoffs", &if old.is_empty() { new } else { format!("{new}\n\n{old}") });
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
        assert!(close_step(&mut n, "Add the client"));
        assert!(!close_step(&mut n, "Add the client"));
        assert_eq!(
            n.body,
            "## Goal\nRefunds through the new provider.\n\n## Steps\n- [x] Add the client\n- [ ] Move refunds\n"
        );
        assert_eq!(goal(&n), "Refunds through the new provider.");
        assert_eq!(steps(&n).iter().filter(|s| !s.done).count(), 1);
    }

    #[test]
    fn the_newest_handoff_comes_first() {
        let mut n = Note::default();
        add_handoff(&mut n, "2026-10-31 09:00Z · web", "Login page done.");
        add_handoff(&mut n, "2026-11-01 14:10Z · api", "Client done.\nTests pass.");
        let h = handoffs(&n);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].heading, "2026-11-01 14:10Z · api");
        assert_eq!(h[0].text, "Client done.\nTests pass.");
        assert_eq!(h[1].text, "Login page done.");
    }
}
