//! Hybrid search: word search (BM25 over the full-text index) and meaning
//! search (vectors) each rank the pieces, the two rankings are merged by
//! reciprocal rank fusion, then binding note types are preferred.

mod query;

pub use query::{QueryKind, classify};

use crate::embed;
use crate::index::Index;
use crate::note::Kind;
use anyhow::Result;
use std::collections::HashMap;

/// Candidates each ranking contributes.
const CANDIDATES: usize = 100;
/// The RRF constant: how quickly lower ranks stop counting.
const RRF_K: f64 = 60.0;
/// A word in more than this share of the pieces says nothing (the, how, is).
const COMMON_WORD: f64 = 0.35;
/// Pieces less similar than this in meaning are not candidates.
const MIN_SIMILARITY: f32 = 0.15;

/// With no word in common, is the best meaning match about the query, or
/// just the least unrelated note? It must be close, or close and far ahead
/// of the rest (z: standard deviations above the mean similarity). Measured
/// on bench/quality.py: topics memory holds, also asked in Turkish, pass;
/// topics it does not hold do not.
fn meaning_is_enough(m: &Meaning) -> bool {
    m.top >= 0.42 || (m.top >= 0.36 && m.top_z >= 4.0)
}

/// The meaning ranking and how clearly its best piece stands out.
struct Meaning {
    ranked: Vec<(i64, f32)>,
    top: f32,
    top_z: f32,
}

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
}

pub fn find(index: &Index, query: &str) -> Result<Vec<Hit>> {
    let qk = classify(query);
    let (w_words, w_meaning) = match qk {
        QueryKind::ErrorLine | QueryKind::Code => (1.5, 0.6),
        QueryKind::Sentence => (0.8, 1.3),
        QueryKind::Words => (1.0, 1.0),
    };
    let mut score: HashMap<i64, f64> = HashMap::new();
    for (rank, rowid) in word_ranking(index, query)?.into_iter().enumerate() {
        *score.entry(rowid).or_default() += w_words / (RRF_K + rank as f64);
    }
    let meaning = meaning_ranking(index, query)?;
    if std::env::var_os("PG_EXPLAIN").is_some() {
        eprintln!(
            "explain: {qk:?}, words {:?}, word hits {}, top similarity {:.3} (z {:.2})",
            query::words(query),
            score.len(),
            meaning.top,
            meaning.top_z
        );
    }
    if score.is_empty() && !meaning_is_enough(&meaning) {
        return Ok(Vec::new()); // nothing in memory is about this
    }
    for (rank, (rowid, _)) in meaning.ranked.into_iter().enumerate() {
        *score.entry(rowid).or_default() += w_meaning / (RRF_K + rank as f64);
    }
    let mut hits = Vec::with_capacity(score.len());
    let mut st = index.db.prepare_cached(
        "SELECT c.path, c.ref, c.kind, c.heading, c.text, n.status FROM chunks c JOIN notes n ON n.path = c.path WHERE c.rowid = ?1",
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
            })
        })?;
        hits.push(h);
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    Ok(hits)
}

/// Rules and decisions bind the work; errors carry fixes; plain notes last.
fn kind_weight(k: Kind) -> f64 {
    match k {
        Kind::Rule | Kind::Decision => 1.25,
        Kind::Error => 1.1,
        Kind::Project => 1.0,
        Kind::Note => 0.9,
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

/// Pieces by BM25 over the query's words (prefix matched), title weighed
/// over heading over text. Words most pieces contain are left out.
fn word_ranking(index: &Index, query: &str) -> Result<Vec<i64>> {
    let total = index.count("SELECT COUNT(*) FROM chunks").max(1) as f64;
    let mut df = index.db.prepare_cached("SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH ?1")?;
    let words: Vec<String> = query::words(query)
        .into_iter()
        .filter(|w| {
            df.query_row([format!("\"{w}\"*")], |r| r.get::<_, i64>(0)).unwrap_or(0) as f64 / total <= COMMON_WORD
        })
        .collect();
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let expr = words.iter().map(|w| format!("\"{w}\"*")).collect::<Vec<_>>().join(" OR ");
    let mut st = index.db.prepare_cached(
        "SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ?1 ORDER BY bm25(chunks_fts, 4.0, 2.0, 1.0) LIMIT ?2",
    )?;
    let rows = st.query_map(rusqlite::params![expr, CANDIDATES as i64], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<i64>>>()?)
}

/// Pieces by the similarity of their vector to the query's.
fn meaning_ranking(index: &Index, query: &str) -> Result<Meaning> {
    let mut m = Meaning { ranked: Vec::new(), top: 0.0, top_z: 0.0 };
    let q = embed::encoder().encode(query);
    if q.iter().all(|x| *x == 0.0) {
        return Ok(m);
    }
    let mut st = index.db.prepare_cached("SELECT rowid, vec FROM chunks")?;
    let mut scored: Vec<(i64, f32)> = st
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
        .flatten()
        .map(|(id, v)| (id, embed::similarity(&q, &embed::from_bytes(&v))))
        .collect();
    if !scored.is_empty() {
        let n = scored.len() as f32;
        let mean = scored.iter().map(|s| s.1).sum::<f32>() / n;
        let sd = (scored.iter().map(|s| (s.1 - mean).powi(2)).sum::<f32>() / n).sqrt();
        m.top = scored.iter().map(|s| s.1).fold(f32::MIN, f32::max);
        m.top_z = (m.top - mean) / sd.max(1e-6);
    }
    scored.retain(|s| s.1 >= MIN_SIMILARITY);
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.truncate(CANDIDATES);
    m.ranked = scored;
    Ok(m)
}
