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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn hint(key: &str, weight: u32, text: &str) -> Hint {
        Hint { key: key.into(), weight, text: text.into(), why: String::new() }
    }

    #[test]
    fn weightiest_first_never_twice_within_budget() {
        let said: HashSet<String> = ["old".to_string()].into();
        let out = choose(
            vec![
                hint("low", 10, "low"),
                hint("old", 99, "said before"),
                hint("high", 90, "high"),
                hint("high", 80, "the same again"),
                hint("long", 50, &"word ".repeat(200)),
            ],
            &said,
            30,
        );
        let keys: Vec<&str> = out.iter().map(|h| h.key.as_str()).collect();
        assert_eq!(keys, ["high", "low"]);
    }

    proptest! {
        /// Any hints: within the budget, none said before, no key twice,
        /// heavier never after lighter, and the same answer every time.
        #[test]
        fn any_hints(raw in prop::collection::vec(("[a-e]", 0..100u32, "[a-z ]{0,80}"), 0..30),
                     said in prop::collection::hash_set("[a-e]", 0..3), tokens in 0..200usize) {
            let hints: Vec<Hint> = raw.iter().map(|(k, w, t)| hint(k, *w, t)).collect();
            let out = choose(hints.clone(), &said, tokens);
            let cost: usize = out.iter().map(|h| crate::output::tokens(&h.text) + 1).sum();
            prop_assert!(cost <= tokens);
            let mut keys = HashSet::new();
            for h in &out {
                prop_assert!(!said.contains(&h.key));
                prop_assert!(keys.insert(h.key.clone()));
            }
            prop_assert!(out.windows(2).all(|w| w[0].weight >= w[1].weight));
            let mut shuffled = hints;
            shuffled.reverse();
            prop_assert_eq!(choose(shuffled, &said, tokens), out);
        }
    }
}
