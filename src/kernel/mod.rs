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
//! agent's context, says nothing twice and keeps to the budget, weighing
//! each kind by the phase of the conversation (its start, its work, its
//! handoff). The gate (gate.rs) says what each expert may do: say, or
//! write inside memory (learn.rs), or nothing.

pub mod adapt;
pub mod attention;
pub mod experts;
pub mod gate;
pub mod learn;
pub mod rank;
pub mod tidy;
pub mod watch;

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
    /// Notes that may be outdated, (id, why), as the last sleep found.
    pub outdated: Vec<(String, String)>,
    /// What could be tidier, as the last sleep found.
    pub tidy: Vec<String>,
    /// The hints said to every conversation lately (to learn what helps).
    pub said_log: Vec<adapt::Said>,
    /// The plan's open steps: number (as pg finish --done takes it), text.
    pub steps: Vec<(usize, String)>,
    /// Drafts pg wrote and nobody confirmed: id, when written.
    pub drafts: Vec<(String, i64)>,
    /// Archived notes: id, path in the project.
    pub archived: Vec<(String, String)>,
    /// Notes read in full lately: id, conversation, when.
    pub reads: Vec<(String, String, i64)>,
    /// Notes with a due date: id, title, date.
    pub dues: Vec<(String, String, String)>,
}

/// Where a conversation is: what is worth its attention changes with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Its brief: the plan, the handoffs, what others did.
    Start,
    /// At work: the code, its errors, the others changing it.
    Middle,
    /// Its handoff: what to say in it, what to close, what to tidy.
    End,
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
/// A brief again after compacting is the middle of the work.
pub fn brief_hints(f: &Facts, tokens: usize) -> Vec<Hint> {
    let mut hints = experts::cut_short(f);
    hints.extend(experts::told(f));
    hints.extend(experts::at_work(f));
    hints.extend(experts::recipes(f));
    hints.extend(experts::outdated(f));
    hints.extend(experts::tidier(f));
    hints.extend(watch::stuck_steps(f));
    hints.extend(watch::stale_drafts(f));
    hints.extend(watch::archive_revival(f));
    hints.extend(watch::due_soon(f));
    let phase = if f.compacted { Phase::Middle } else { Phase::Start };
    decide(f, hints, phase, tokens)
}

/// What any command adds: notes the other conversations left, and what
/// this conversation's own work calls for (a plan, a handoff), once.
pub fn news_hints(f: &Facts, tokens: usize) -> Vec<Hint> {
    let mut hints = experts::told(f);
    hints.extend(watch::unplanned_work(f));
    hints.extend(watch::long_work(f));
    decide(f, hints, Phase::Middle, tokens)
}

/// What `pg finish` adds: what could be tidier, drafts waiting, once.
pub fn finish_hints(f: &Facts, tokens: usize) -> Vec<Hint> {
    let mut hints = experts::tidier(f);
    hints.extend(watch::stale_drafts(f));
    decide(f, hints, Phase::End, tokens)
}

/// The hints the gate lets say, weighed by how their kind fared and by the
/// phase, chosen to fit.
pub fn decide(f: &Facts, hints: Vec<Hint>, phase: Phase, tokens: usize) -> Vec<Hint> {
    let hints: Vec<Hint> = hints.into_iter().filter(|h| gate::may_say(adapt::kind(&h.key))).collect();
    let hints = adapt::adapt(hints, &adapt::usage(f, &f.said_log));
    attention::choose(attention::by_phase(hints, phase), &f.said, tokens)
}
