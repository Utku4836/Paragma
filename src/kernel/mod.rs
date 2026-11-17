//! The pure core: what pg says, decided from facts alone.
//!
//! A command's thin shell gathers the facts (perception: the event log,
//! the runs, what this conversation was already told) and prints what the
//! core decides. The core reads no file, no clock and no database: given
//! the same facts it says the same thing, so every rule here is tested on
//! its own, with any input.
//!
//! ```text
//! shell:  perceive -> Facts
//! core:   experts(Facts) -> hints  ->  attention(hints, already said, budget) -> lines
//! shell:  print, remember what was said
//! ```
//!
//! An expert is one small rule, "this fact means that": a conversation cut
//! short, the commands that work here, the change that fixed a test. Experts
//! do not know each other; attention alone chooses what is worth the
//! agent's context, says nothing twice and keeps to the budget.

pub mod attention;
pub mod experts;
pub mod rank;

use crate::state::journal::Entry;
use crate::testrun::Run;
use std::collections::{HashMap, HashSet};

/// What pg knows when it decides what to say.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// Now, in seconds since the epoch.
    pub now: i64,
    /// This conversation.
    pub me: String,
    /// The event log of the last days, oldest first, of every conversation.
    pub journal: Vec<Entry>,
    /// The runs of commands here, newest first.
    pub runs: Vec<Run>,
    /// The folder each conversation worked in, by conversation.
    pub folders: HashMap<String, String>,
    /// This conversation was compacted and gets its brief again.
    pub compacted: bool,
    /// Hints already said to this conversation, by key.
    pub said: HashSet<String>,
}

/// Something worth telling the agent, with why.
#[derive(Clone, Debug, PartialEq)]
pub struct Hint {
    /// Said once per conversation: the same key is not said again.
    pub key: String,
    /// How much it matters: higher first.
    pub weight: u32,
    pub text: String,
    /// Why it is said (for PG_EXPLAIN and tests).
    pub why: String,
}

/// Every expert's hints for the brief, chosen by attention to fit `tokens`.
pub fn brief_hints(f: &Facts, tokens: usize) -> Vec<Hint> {
    let mut hints = experts::cut_short(f);
    hints.extend(experts::at_work(f));
    hints.extend(experts::recipes(f));
    attention::choose(hints, &f.said, tokens)
}
