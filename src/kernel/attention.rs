//! Attention: of everything the experts would say, what is said. One place
//! decides, so no command floods the agent's context: the weightiest first,
//! nothing already said in this conversation, nothing twice, within the
//! budget.

use super::{Hint, Phase};
use std::collections::HashSet;

/// How much a kind of hint weighs in each phase, in percent (100 when not
/// named): at the start the plan and what others did, at work the code and
/// the others changing it, at the handoff what to tidy and say in it.
const PHASE: &[(&str, Phase, u32)] = &[
    ("stuck", Phase::Start, 120),
    ("revive", Phase::Start, 110),
    ("stuck", Phase::Middle, 50),
    ("drafts", Phase::Middle, 50),
    ("tidy", Phase::Middle, 50),
    ("revive", Phase::Middle, 60),
    ("recipes", Phase::Middle, 80),
    ("told", Phase::Middle, 110),
    ("tidy", Phase::End, 130),
    ("drafts", Phase::End, 130),
    ("thin-handoff", Phase::End, 130),
    ("recipes", Phase::End, 50),
    ("work", Phase::End, 60),
];

/// The hints weighed for the phase.
pub fn by_phase(mut hints: Vec<Hint>, phase: Phase) -> Vec<Hint> {
    for h in &mut hints {
        let kind = super::adapt::kind(&h.key);
        if let Some((_, _, pct)) = PHASE.iter().find(|(k, p, _)| *k == kind && *p == phase) {
            h.weight = (h.weight * pct / 100).max(1);
        }
    }
    hints
}

pub fn choose(mut hints: Vec<Hint>, said: &HashSet<String>, tokens: usize) -> Vec<Hint> {
    // a fixed order: the same facts give the same lines
    hints.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.key.cmp(&b.key)).then_with(|| a.text.cmp(&b.text)));
    let mut out: Vec<Hint> = Vec::new();
    let mut left = tokens;
    for h in hints {
        if said.contains(&h.key) || out.iter().any(|o| o.key == h.key) {
            continue;
        }
        let cost = crate::output::tokens(&h.text) + 1;
        if cost > left {
            continue; // a shorter one further down may still fit
        }
        left -= cost;
        out.push(h);
    }
    out
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/kernel/attention.rs"]
mod tests;
