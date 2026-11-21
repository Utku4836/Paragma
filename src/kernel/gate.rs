//! The safety gate: what each expert may do. An expert that writes
//! (level 1) does so only while it earns it:
//! - on the scenario sets it must reach a full score, else it is set to
//!   say what it would write (level 0) in `EXPERTS`;
//! - here, when the agents take back (`pg undo`) a third or more of what it
//!   wrote, after `UNDONE_AFTER` writes, it says instead of writing.
//!
//! An expert that only says and misfires on the sets is turned off in
//! `EXPERTS`; at work, adapt.rs makes one seldom acted on quieter.

use super::Facts;
use crate::state::journal::Did;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Says nothing.
    Off,
    /// Says, with how to act on it.
    Say,
    /// Does it, inside memory, undoably.
    Do,
}

/// Every expert, the name its actions and hints go by, and its level as
/// the scenario sets left it.
pub const EXPERTS: &[(&str, Level)] = &[
    // they write
    ("handoff", Level::Do),
    ("draft", Level::Do),
    ("rule-draft", Level::Do),
    ("flaky", Level::Do),
    ("outcome", Level::Do),
    ("recipe", Level::Do),
    // they say
    ("cut", Level::Say),
    ("so-far", Level::Say),
    ("told", Level::Say),
    ("work", Level::Say),
    ("recipes", Level::Say),
    ("outdated", Level::Say),
    ("tidy", Level::Say),
    ("conflict", Level::Say),
    ("taken-open", Level::Say),
    ("stuck", Level::Say),
    ("drafts", Level::Say),
    ("revive", Level::Say),
    ("thin-handoff", Level::Say),
    ("guard", Level::Say),
];

/// Writes before the undo share counts.
pub const UNDONE_AFTER: usize = 3;

/// The level an expert was given.
pub fn level(name: &str) -> Level {
    EXPERTS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l).unwrap_or(Level::Off)
}

/// Whether an expert that writes may write now: its level, and what the
/// agents did with what it wrote lately (taken back a third or more of the
/// time: it says instead).
pub fn may_write(f: &Facts, name: &str) -> bool {
    level(name) == Level::Do && !undone_often(f, name)
}

/// Whether its writes were taken back a third of the time or more.
pub fn undone_often(f: &Facts, name: &str) -> bool {
    let undone: HashSet<i64> =
        f.journal.iter().filter_map(|e| if let Did::Undone { of } = e.did { Some(of) } else { None }).collect();
    let mine: Vec<i64> =
        f.journal.iter().filter(|e| matches!(&e.did, Did::Auto { what, .. } if what == name)).map(|e| e.seq).collect();
    let back = mine.iter().filter(|s| undone.contains(s)).count();
    mine.len() >= UNDONE_AFTER && back * 3 >= mine.len()
}

/// Whether an expert that says may say.
pub fn may_say(name: &str) -> bool {
    level(name) != Level::Off
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/kernel/gate.rs"]
mod tests;
