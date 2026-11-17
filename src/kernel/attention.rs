//! Attention: of everything the experts would say, what is said. One place
//! decides, so no command floods the agent's context: the weightiest first,
//! nothing already said in this conversation, nothing twice, within the
//! budget.

use super::Hint;
use std::collections::HashSet;

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
#[path = "../../unit/src/kernel/attention.rs"]
mod tests;
