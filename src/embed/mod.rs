//! Text to meaning vectors with the embedded Model2Vec model.
//!
//! A static model: every token has a fixed vector and a text's vector is the
//! mean of its tokens' vectors, each weighted by how much information it
//! carries (frequent pieces like "the" or "de" count little), normalised. The model files
//! (built by tools/build-model) are compiled into the binary and read in
//! place, so loading costs a few milliseconds.

mod tokenize;

use std::sync::OnceLock;

static VOCAB: &[u8] = include_bytes!("../../model/vocab.fst");
static SCORES: &[u8] = include_bytes!("../../model/scores.f32");
static TABLE: &[u8] = include_bytes!("../../model/table.bin");
static SCALES: &[u8] = include_bytes!("../../model/scales.f32");
static META: &str = include_str!("../../model/meta.json");

/// SIF weighting (Arora et al. 2017): a token counts a / (a + p), p its
/// probability. Measured with bench/quality.py: 0.001 beats no weighting
/// and 0.0001.
const SIF_A: f32 = 0.001;

pub struct Encoder {
    vocab: fst::Map<&'static [u8]>,
    unk: u32,
    min_score: f32,
    dim: usize,
    dtype: Dtype,
}

/// How the table stores a value: f16, or i8 / i4 times a scale per row.
#[derive(Clone, Copy, PartialEq)]
enum Dtype {
    F16,
    I8,
    I4,
}

/// Which model the vectors come from: an index built with another one is
/// rebuilt.
pub fn fingerprint() -> String {
    let m: serde_json::Value = serde_json::from_str(META).unwrap_or_default();
    format!(
        "{}-{}x{}-{}-sif{SIF_A}",
        m["source"].as_str().unwrap_or(""),
        m["rows"],
        m["dim"],
        m["dtype"].as_str().unwrap_or("f16"),
    )
}

/// The encoder, built on first use.
pub fn encoder() -> &'static Encoder {
    static E: OnceLock<Encoder> = OnceLock::new();
    E.get_or_init(Encoder::load)
}

impl Encoder {
    fn load() -> Encoder {
        let meta: serde_json::Value = serde_json::from_str(META).expect("model meta.json");
        let dim = meta["dim"].as_u64().expect("model dim") as usize;
        let dtype = match meta["dtype"].as_str() {
            Some("i8") => Dtype::I8,
            Some("i4") => Dtype::I4,
            _ => Dtype::F16,
        };
        let unk = meta["unk_id"].as_u64().unwrap_or(0) as u32;
        let min_score =
            (0..SCORES.len() / 4).filter(|&i| i as u32 != unk).map(|i| score_at(i as u32)).fold(f32::MAX, f32::min);
        Encoder { vocab: fst::Map::new(VOCAB).expect("model vocab.fst"), unk, min_score, dim, dtype }
    }

    /// The text's vector, unit length (all zeros for a text with no known token).
    pub fn encode(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0f32; self.dim];
        let mut total = 0f32;
        for id in tokenize::ids(self, text) {
            if id <= self.unk {
                continue; // padding and unknown tokens carry no meaning
            }
            let w = self.weight(id);
            self.add_row(id as usize, w, &mut v);
            total += w;
        }
        if total == 0.0 {
            return vec![0f32; self.dim];
        }
        normalize(&mut v);
        v
    }

    /// How much a token counts: a / (a + p), p its probability. Rare tokens
    /// carry more information (-log p) and count nearly fully; the most
    /// frequent ones count little.
    fn weight(&self, id: u32) -> f32 {
        let p = score_at(id).exp();
        SIF_A / (SIF_A + p)
    }

    fn add_row(&self, id: usize, w: f32, v: &mut [f32]) {
        let d = self.dim;
        let scale = || w * f32::from_le_bytes(SCALES[id * 4..id * 4 + 4].try_into().expect("4 bytes"));
        match self.dtype {
            Dtype::F16 => {
                for (x, b) in v.iter_mut().zip(TABLE[id * d * 2..(id + 1) * d * 2].as_chunks::<2>().0) {
                    *x += w * half::f16::from_le_bytes(*b).to_f32();
                }
            }
            Dtype::I8 => {
                let s = scale();
                for (x, b) in v.iter_mut().zip(&TABLE[id * d..(id + 1) * d]) {
                    *x += *b as i8 as f32 * s;
                }
            }
            Dtype::I4 => {
                // two values per byte, low nibble first, each stored as value + 8
                let s = scale();
                let row = &TABLE[id * d.div_ceil(2)..(id + 1) * d.div_ceil(2)];
                for (i, x) in v.iter_mut().enumerate() {
                    let b = row[i / 2];
                    let nibble = if i % 2 == 0 { b & 0x0F } else { b >> 4 };
                    *x += (nibble as i8 - 8) as f32 * s;
                }
            }
        }
    }

    /// The vocabulary's pieces the text starts with, as (length in
    /// characters, id, score): one walk down the vocabulary's automaton,
    /// which stops as soon as no piece starts that way.
    fn prefixes(&self, chars: &[char], out: &mut Vec<(usize, u32, f32)>) {
        out.clear();
        let fst = self.vocab.as_fst();
        let mut node = fst.root();
        let mut output = fst::raw::Output::zero();
        let mut utf8 = [0u8; 4];
        for (k, c) in chars.iter().enumerate() {
            for b in c.encode_utf8(&mut utf8).bytes() {
                let Some(i) = node.find_input(b) else { return };
                let t = node.transition(i);
                output = output.cat(t.out);
                node = fst.node(t.addr);
            }
            if node.is_final() {
                let id = output.cat(node.final_output()).value() as u32;
                out.push((k + 1, id, score_at(id)));
            }
        }
    }
}

fn score_at(id: u32) -> f32 {
    let o = id as usize * 4;
    f32::from_le_bytes([SCORES[o], SCORES[o + 1], SCORES[o + 2], SCORES[o + 3]])
}

pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
}

/// Cosine similarity of two unit vectors.
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Vectors are stored as little-endian f32 bytes.
pub fn to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub fn from_bytes(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn related_texts_are_close() {
        let e = encoder();
        let refund = e.encode("Refunds are allowed only within 30 days of the payment.");
        let tr = e.encode("para iadesi ödemeden sonra kaç gün içinde yapılabilir");
        let other = e.encode("The shader compiles the vertex buffer on the GPU.");
        assert!(similarity(&refund, &tr) > similarity(&other, &tr));
        assert!((similarity(&refund, &refund) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn unknown_text_is_zero() {
        assert!(encoder().encode("").iter().all(|x| *x == 0.0));
    }
}
