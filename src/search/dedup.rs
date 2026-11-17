//! Near duplicates fold into one result: the same mistake recorded in many
//! packages, or the same decision template, shows once with a count, so the
//! budget goes to different things.

use super::Hit;
use crate::embed;

/// Results at least this similar in meaning (and of the same kind) are the
/// same thing said again.
pub const SAME_THING: f32 = 0.90;

/// How many results are compared; below these nothing is shown anyway.
const LOOKED_AT: usize = 300;

/// Keeps the best of each group of near duplicates, in score order, each
/// with the number of others it stands for.
pub fn fold(hits: Vec<Hit>) -> Vec<Hit> {
    let mut kept: Vec<Hit> = Vec::new();
    for h in hits.into_iter().take(LOOKED_AT) {
        let twin = kept.iter_mut().find(|k| k.kind == h.kind && embed::similarity(&k.vec, &h.vec) >= SAME_THING);
        match twin {
            Some(k) => k.similar += 1,
            None => kept.push(h),
        }
    }
    kept
}

#[cfg(all(test, paragma_local))]
#[path = "../../unit/src/search/dedup.rs"]
mod tests;
