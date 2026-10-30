//! The two rankings hybrid search merges: by words (BM25) and by meaning
//! (vector similarity).

use super::query;
use crate::embed;
use crate::index::Index;
use anyhow::Result;

/// Candidates each ranking contributes.
pub const CANDIDATES: usize = 100;
/// A word in more than this share of the pieces says nothing (the, how, is).
const COMMON_WORD: f64 = 0.35;
/// Pieces less similar than this in meaning are not candidates.
const MIN_SIMILARITY: f32 = 0.15;

/// Pieces by BM25 over the query's words (prefix matched), title weighed
/// over heading over text. Words most pieces contain are left out.
pub fn by_words(index: &Index, query: &str) -> Result<Vec<i64>> {
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

/// The meaning ranking and how clearly its best piece stands out.
pub struct Meaning {
    pub ranked: Vec<(i64, f32)>,
    pub top: f32,
    /// How many standard deviations the best similarity is above the mean.
    pub top_z: f32,
}

impl Meaning {
    /// With no word in common, is the best match about the query, or just
    /// the least unrelated note? It must be close, or close and far ahead of
    /// the rest. Measured on bench/quality.py: topics memory holds, also
    /// asked in other languages, pass; topics it does not hold do not.
    pub fn is_enough(&self) -> bool {
        self.top >= 0.42 || (self.top >= 0.36 && self.top_z >= 4.0)
    }
}

/// Pieces by the similarity of their vector to the query's.
pub fn by_meaning(index: &Index, query: &str) -> Result<Meaning> {
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
