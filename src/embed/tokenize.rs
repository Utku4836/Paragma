//! The model's tokenizer: the same normalisation and Unigram best path as
//! the `tokenizers` library uses for this model, so the vectors match the
//! official Model2Vec ones.

use super::Encoder;
use unicode_normalization::UnicodeNormalization;

/// Longest piece tried, in characters (longer pieces are names and URLs the
/// model rarely has).
const MAX_PIECE: usize = 32;

/// `tokenizers` gives an unknown character this much below the lowest score.
const UNK_PENALTY: f32 = 10.0;

/// Token ids of a text.
pub fn ids(enc: &Encoder, text: &str) -> Vec<u32> {
    let chars = normalize(text);
    let n = chars.len();
    // best[i] = (score, start of the last piece, its id) of the best path to i
    let mut best: Vec<(f32, usize, u32)> = vec![(f32::NEG_INFINITY, 0, 0); n + 1];
    best[0].0 = 0.0;
    let mut found = Vec::new();
    for i in 0..n {
        if best[i].0 == f32::NEG_INFINITY {
            continue;
        }
        let mut single = false;
        enc.prefixes(&chars[i..(i + MAX_PIECE).min(n)], &mut found);
        for &(len, id, score) in &found {
            let j = i + len - 1;
            single |= len == 1;
            let s = best[i].0 + score;
            if s > best[j + 1].0 {
                best[j + 1] = (s, i, id);
            }
        }
        if !single {
            let s = best[i].0 + enc.min_score - UNK_PENALTY;
            if s > best[i + 1].0 {
                best[i + 1] = (s, i, enc.unk);
            }
        }
    }
    let mut out = Vec::new();
    let mut at = n;
    while at > 0 {
        out.push(best[at].2);
        at = best[at].1;
    }
    out.reverse();
    out
}

/// NFKC, ASCII punctuation set apart, whitespace collapsed, then Metaspace:
/// every word starts with '▁'.
fn normalize(text: &str) -> Vec<char> {
    let mut s = String::with_capacity(text.len() + 8);
    for c in text.nfkc() {
        if c.is_ascii_punctuation() {
            s.push(' ');
            s.push(c);
            s.push(' ');
        } else {
            s.push(c);
        }
    }
    let mut out = Vec::with_capacity(s.len() + 1);
    for word in s.split_whitespace() {
        out.push('\u{2581}');
        out.extend(word.chars());
    }
    out
}
