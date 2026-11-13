//! Hybrid search: word search (BM25 over the full-text index) and meaning
//! search (vectors) each rank the pieces, the two rankings are merged by
//! reciprocal rank fusion, binding note types are preferred, and near
//! duplicates fold into one result.

mod dedup;
mod glossary;
pub mod graph;
pub mod query;
mod rank;

pub use query::words as words_of;
pub use query::{QueryKind, classify};

use crate::embed;
use crate::index::Index;
use crate::note::Kind;
use anyhow::Result;
use std::collections::HashMap;

/// The RRF constant: how quickly lower ranks stop counting.
const RRF_K: f64 = 60.0;

#[derive(Clone, Debug)]
pub struct Hit {
    /// Where the note is, in the project.
    pub path: String,
    /// The piece's id: the note id, or a rule id.
    pub id: String,
    pub kind: Kind,
    pub heading: String,
    pub text: String,
    pub score: f64,
    /// The piece's meaning vector.
    pub vec: Vec<f32>,
    /// Near duplicates folded into this result.
    pub similar: usize,
}

pub fn find(index: &Index, query: &str) -> Result<Vec<Hit>> {
    // every query below reads one snapshot: a parallel write replacing a
    // note's pieces meanwhile must not make a found piece vanish
    let _snapshot = index.db.unchecked_transaction()?;
    let qk = classify(query);
    let (w_words, w_meaning) = match qk {
        QueryKind::ErrorLine | QueryKind::Code => (1.5, 0.6),
        QueryKind::Sentence => (0.8, 1.3),
        QueryKind::Words => (1.0, 1.0),
    };
    let mut score: HashMap<i64, f64> = HashMap::new();
    for (rank, rowid) in rank::by_words(index, query)?.into_iter().enumerate() {
        *score.entry(rowid).or_default() += w_words / (RRF_K + rank as f64);
    }
    let meaning = rank::by_meaning(index, query)?;
    if std::env::var_os("PG_EXPLAIN").is_some() {
        eprintln!(
            "explain: {qk:?}, words {:?}, word hits {}, top similarity {:.3} (z {:.2})",
            query::words(query),
            score.len(),
            meaning.top,
            meaning.top_z
        );
        let mut st = index.db.prepare_cached("SELECT ref FROM chunks WHERE rowid = ?1")?;
        let top: Vec<String> = meaning
            .ranked
            .iter()
            .take(12)
            .map(|(id, s)| format!("{}:{s:.2}", st.query_row([id], |r| r.get::<_, String>(0)).unwrap_or_default()))
            .collect();
        eprintln!("explain: by meaning {}", top.join(" "));
    }
    if score.is_empty() && !meaning.is_enough() {
        return Ok(Vec::new()); // nothing in memory is about this
    }
    for (rank, (rowid, _)) in meaning.ranked.into_iter().enumerate() {
        *score.entry(rowid).or_default() += w_meaning / (RRF_K + rank as f64);
    }
    let mut hits = Vec::with_capacity(score.len());
    let mut st = index.db.prepare_cached(
        "SELECT c.path, c.ref, c.kind, c.heading, c.text, n.status, c.vec FROM chunks c JOIN notes n ON n.path = c.path WHERE c.rowid = ?1",
    )?;
    for (rowid, s) in score {
        let h = st.query_row([rowid], |r| {
            let kind = Kind::parse(&r.get::<_, String>(2)?);
            let status: String = r.get::<_, Option<String>>(5)?.unwrap_or_default();
            Ok(Hit {
                path: r.get(0)?,
                id: r.get(1)?,
                kind,
                heading: r.get(3)?,
                text: r.get(4)?,
                score: s * kind_weight(kind) * status_weight(&status),
                vec: embed::from_bytes(&r.get::<_, Vec<u8>>(6)?),
                similar: 0,
            })
        });
        hits.push(h?);
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    Ok(dedup::fold(hits))
}

/// Moves error records by how their fixes did when shown before
/// (state/outcome.rs): at most a fifth either way, and not at all before
/// two outcomes. Then sorts again.
pub fn weigh_outcomes(hits: &mut [Hit], state: &crate::state::State) {
    if !state.has_outcomes() {
        return; // nothing learned yet: one small read
    }
    for h in hits.iter_mut().filter(|h| h.kind == Kind::Error) {
        if let Ok(t) = state.track(&h.id) {
            h.score *= t.factor();
        }
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
}

/// Every kind of record weighs the same: the one that answers wins, a note
/// as much as a decision (measured on real notes: preferring rules and
/// decisions put the note that answered below them). The project note
/// touches every topic of the work (goal, steps, handoffs): it comes after
/// the record about the topic itself.
fn kind_weight(k: Kind) -> f64 {
    match k {
        Kind::Project => 0.7,
        Kind::Rule | Kind::Decision | Kind::Error | Kind::Note => 1.0,
    }
}

fn status_weight(status: &str) -> f64 {
    match status {
        "superseded" => 0.5,
        "archived" => 0.4,
        "stale" => 0.8,
        _ => 1.0,
    }
}
