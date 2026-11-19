//! Adaptation: pg learns from what the agent did with what it was told.
//!
//! - Each kind of hint keeps how often it was said and how often the agent
//!   acted on it after (ran a recipe, confirmed or fixed an outdated note,
//!   tidied). A kind seldom acted on gets quieter, one often acted on louder;
//!   never silent (a quiet kind is still said when there is room).
//! - A note's liveness comes from all its uses, each fading with time
//!   (ACT-R's base-level activation, Anderson 1993): a note used often stays
//!   alive long after its last use, one used once fades like any memory.

use super::{Facts, Hint};
use crate::state::journal::Did;
use std::collections::HashMap;

/// Said this often before what it does with it counts.
pub const LEARN_AFTER: usize = 3;
/// How fast a use fades (ACT-R's usual d).
pub const DECAY: f64 = 0.5;

/// A hint said to a conversation: whose, which, when.
#[derive(Clone, Debug, PartialEq)]
pub struct Said {
    pub conversation: String,
    pub key: String,
    pub at: i64,
}

/// The kind of a hint: its key before ':'.
pub fn kind(key: &str) -> &str {
    key.split(':').next().unwrap_or(key)
}

/// Whether a conversation acted on a hint of this kind after `at`: what it
/// did next in the log.
fn acted(f: &Facts, conversation: &str, kind: &str, at: i64) -> Option<bool> {
    let after = f.journal.iter().filter(|e| e.conversation == conversation && e.at >= at);
    match kind {
        "recipes" => Some(after.into_iter().any(|e| {
            matches!(&e.did, Did::Run { command, .. } if super::experts::purpose(command) != super::experts::Purpose::Other
                || f.runs.iter().any(|r| r.command == *command && (r.passed > 0 || !r.failing.is_empty())))
        })),
        "outdated" => Some(after.into_iter().any(|e| {
            matches!(&e.did, Did::Write { kind, .. } if kind == "confirm" || kind == "error" || kind == "decision")
                || matches!(&e.did, Did::Rm { .. })
        })),
        "tidy" => Some(after.into_iter().any(|e| matches!(&e.did, Did::Mv { .. } | Did::Rm { .. }))),
        // told, cut short, at work, a known fix (state/outcome.rs keeps its own)
        _ => None,
    }
}

/// For each kind of hint that can be followed: (said, acted on).
pub fn usage(f: &Facts, said: &[Said]) -> HashMap<String, (usize, usize)> {
    let mut out: HashMap<String, (usize, usize)> = HashMap::new();
    for s in said {
        let k = kind(&s.key);
        if let Some(did) = acted(f, &s.conversation, k, s.at) {
            let e = out.entry(k.to_string()).or_default();
            e.0 += 1;
            e.1 += usize::from(did);
        }
    }
    out
}

/// Each hint's weight times how its kind fared: (acted + 1) / (said + 2),
/// around 1 for a kind not known yet, between 0.5 and 1.5.
pub fn adapt(mut hints: Vec<Hint>, usage: &HashMap<String, (usize, usize)>) -> Vec<Hint> {
    for h in &mut hints {
        if let Some(&(said, acted)) = usage.get(kind(&h.key))
            && said >= LEARN_AFTER
        {
            let rate = (acted as f64 + 1.0) / (said as f64 + 2.0);
            h.weight = ((h.weight as f64) * (0.5 + rate)).round().max(1.0) as u32;
            h.why = format!("{} (acted on {acted} of {said} times)", h.why);
        }
    }
    hints
}

/// Uses closer than this are one use (a note read and written in the same
/// sitting, its write and its file's time).
const ONE_SITTING: i64 = 3600;

/// ACT-R base-level activation of something used at these times: the log
/// of the sum, over its uses, of the days since each to the power -DECAY.
/// A use counts as at least an hour old.
pub fn activation(uses: &[i64], now: i64) -> f64 {
    let mut times: Vec<i64> = uses.to_vec();
    times.sort_unstable();
    let mut sum = 0.0;
    let mut last: Option<i64> = None;
    for t in times {
        if last.is_some_and(|l| t - l < ONE_SITTING) {
            continue;
        }
        last = Some(t);
        sum += ((now - t).max(3600) as f64 / 86_400.0).powf(-DECAY);
    }
    if sum > 0.0 { sum.ln() } else { f64::NEG_INFINITY }
}

/// Whether something is as alive as a thing used once `days` ago, or more.
pub fn alive(uses: &[i64], now: i64, days: i64) -> bool {
    activation(uses, now) >= activation(&[now - days * 86_400], now)
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/kernel/adapt.rs"]
mod tests;
