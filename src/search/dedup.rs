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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::Kind;

    fn hit(id: &str, kind: Kind, vec: Vec<f32>) -> Hit {
        Hit {
            path: format!("{id}.md"),
            id: id.into(),
            kind,
            heading: String::new(),
            text: String::new(),
            score: 0.0,
            vec,
            similar: 0,
        }
    }

    #[test]
    fn twins_fold_into_the_first() {
        let hits = vec![
            hit("e1", Kind::Error, vec![1.0, 0.0]),
            hit("r1", Kind::Rule, vec![1.0, 0.0]), // same meaning, other kind: kept
            hit("e2", Kind::Error, vec![0.99, 0.1]), // twin of e1
            hit("e3", Kind::Error, vec![0.0, 1.0]),
        ];
        let out = fold(hits);
        let ids: Vec<_> = out.iter().map(|h| (h.id.as_str(), h.similar)).collect();
        assert_eq!(ids, vec![("e1", 1), ("r1", 0), ("e3", 0)]);
    }
}
