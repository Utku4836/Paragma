//! Builds the embedding model that pg embeds in its binary.
//!
//!   build-model <model2vec-folder> <out-folder> [--keep all|latin] [--top N] [--dims N] [--dtype f16|i8|i4]
//!
//! The input is a Model2Vec model with a Unigram tokenizer (we use
//! minishlab/potion-multilingual-128M; `fetch.sh` downloads it). The output
//! is stored so pg loads it in a few ms and only reads the rows it needs:
//!
//!   vocab.fst    piece -> id (a finite state transducer, read in place)
//!   scores.f32   Unigram score per id
//!   table.bin    one row of `dim` values per id: f16, or i8 / i4 with a scale per row
//!   scales.f32   the scale of each row (empty for f16)
//!   meta.json    rows, dim, dtype, unk_id, how it was built
//!
//! Ways to make it smaller, each measured with bench/quality.py:
//!   --keep latin  only Latin-script pieces (with the punctuation and symbols of Latin text)
//!   --top N       only the N most frequent pieces (single characters always kept)
//!   --dims N      the first N dimensions (Model2Vec stores them by variance)
//!   --dtype i8    one byte per value instead of two; i4: half a byte

use anyhow::{Context, Result, bail};
use std::path::Path;

#[derive(Clone, Copy, PartialEq)]
enum Dtype {
    F16,
    I8,
    I4,
}

impl Dtype {
    fn name(self) -> &'static str {
        match self {
            Dtype::F16 => "f16",
            Dtype::I8 => "i8",
            Dtype::I4 => "i4",
        }
    }

    /// The largest stored value: a row is scaled so its largest value maps to it.
    fn levels(self) -> f32 {
        match self {
            Dtype::F16 => 1.0,
            Dtype::I8 => 127.0,
            Dtype::I4 => 7.0,
        }
    }
}

struct Options {
    latin_only: bool,
    top: Option<usize>,
    dims: Option<usize>,
    dtype: Dtype,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: build-model <model2vec-folder> <out-folder> [--keep all|latin] [--top N] [--dims N] [--dtype f16|i8|i4]";
    let (src, dst) = match (args.first(), args.get(1)) {
        (Some(s), Some(d)) if !s.starts_with("--") && !d.starts_with("--") => (s, d),
        _ => bail!(usage),
    };
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str);
    let number = |name: &str| -> Result<Option<usize>> {
        flag(name).map(|v| v.parse().with_context(|| format!("{name} takes a number"))).transpose()
    };
    let opts = Options {
        latin_only: match flag("--keep").unwrap_or("all") {
            "all" => false,
            "latin" => true,
            _ => bail!(usage),
        },
        top: number("--top")?,
        dims: number("--dims")?,
        dtype: match flag("--dtype").unwrap_or("f16") {
            "f16" => Dtype::F16,
            "i8" => Dtype::I8,
            "i4" => Dtype::I4,
            _ => bail!(usage),
        },
    };
    build(Path::new(src), Path::new(dst), &opts)
}

/// Characters of Latin-script text: the letters of every Latin alphabet
/// (Vietnamese, Romanian, Azerbaijani included), their accents, and the
/// punctuation and currency signs such text uses.
fn is_latin(c: char) -> bool {
    matches!(c,
        '\u{2581}'                  // the tokenizer's word-start mark
        | ' '..='~'                  // ASCII
        | '\u{00A0}'..='\u{024F}'    // Latin-1 Supplement, Latin Extended-A and -B
        | '\u{0259}'                  // schwa (Azerbaijani)
        | '\u{0300}'..='\u{036F}'    // combining accents
        | '\u{1E00}'..='\u{1EFF}'    // Latin Extended Additional (Vietnamese)
        | '\u{2010}'..='\u{205E}'    // dashes, quotes, ellipsis
        | '\u{20A0}'..='\u{20C0}'    // currency signs
    )
}

fn build(src: &Path, dst: &Path, opts: &Options) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    let tokenizer: serde_json::Value = serde_json::from_slice(&std::fs::read(src.join("tokenizer.json"))?)?;
    if tokenizer["model"]["type"] != "Unigram" {
        bail!("only Unigram tokenizers are supported");
    }
    let vocab = tokenizer["model"]["vocab"].as_array().context("tokenizer has no vocab")?;
    let unk = tokenizer["model"]["unk_id"].as_u64().context("tokenizer has no unk_id")? as usize;
    let piece = |i: usize| vocab[i][0].as_str().unwrap_or("");
    let score = |i: usize| vocab[i][1].as_f64().unwrap_or(f64::MIN);

    // pieces kept: special tokens, every single character (so no text
    // becomes unknown), then the most frequent of the rest
    let candidates: Vec<usize> =
        (0..vocab.len()).filter(|&i| i <= unk || !opts.latin_only || piece(i).chars().all(is_latin)).collect();
    let is_char = |i: usize| piece(i).trim_start_matches('\u{2581}').chars().count() <= 1;
    let mut keep: Vec<usize> = candidates.iter().copied().filter(|&i| i <= unk || is_char(i)).collect();
    let mut rest: Vec<usize> = candidates.iter().copied().filter(|&i| i > unk && !is_char(i)).collect();
    rest.sort_by(|&a, &b| score(b).total_cmp(&score(a)));
    let room = opts.top.map(|n| n.saturating_sub(keep.len())).unwrap_or(rest.len());
    keep.extend(rest.into_iter().take(room));
    keep.sort();

    let bytes = std::fs::read(src.join("model.safetensors"))?;
    let tensors = safetensors::SafeTensors::deserialize(&bytes)?;
    let embeddings = tensors.tensor("embeddings")?;
    let full_dim = embeddings.shape()[1];
    let dim = opts.dims.unwrap_or(full_dim).min(full_dim);
    let raw = embeddings.data();
    let value = |i: usize| -> f32 {
        match embeddings.dtype() {
            safetensors::Dtype::F16 => half::f16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]).to_f32(),
            _ => f32::from_le_bytes([raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]]),
        }
    };

    let mut table = Vec::with_capacity(keep.len() * dim * 2);
    let mut scales = Vec::new();
    let mut scores = Vec::with_capacity(keep.len() * 4);
    let mut pieces: Vec<(String, u64)> = Vec::with_capacity(keep.len());
    for (new_id, &old_id) in keep.iter().enumerate() {
        let row: Vec<f32> = (0..dim).map(|j| value(old_id * full_dim + j)).collect();
        match opts.dtype {
            Dtype::F16 => table.extend(row.iter().flat_map(|x| half::f16::from_f32(*x).to_le_bytes())),
            dtype => {
                let max = dtype.levels();
                let scale = row.iter().fold(0f32, |m, x| m.max(x.abs())) / max;
                let scale = if scale > 0.0 { scale } else { 1.0 };
                let q: Vec<i8> = row.iter().map(|x| (x / scale).round().clamp(-max, max) as i8).collect();
                if dtype == Dtype::I8 {
                    table.extend(q.iter().map(|v| *v as u8));
                } else {
                    // two values per byte, low nibble first, each stored as value + 8
                    for pair in q.chunks(2) {
                        let lo = (pair[0] + 8) as u8;
                        let hi = pair.get(1).map(|v| (v + 8) as u8).unwrap_or(8);
                        table.push(lo | (hi << 4));
                    }
                }
                scales.extend_from_slice(&scale.to_le_bytes());
            }
        }
        scores.extend_from_slice(&(score(old_id) as f32).to_le_bytes());
        pieces.push((piece(old_id).to_string(), new_id as u64));
    }
    pieces.sort();
    pieces.dedup_by(|a, b| a.0 == b.0);
    let mut fst = fst::MapBuilder::memory();
    for (p, id) in &pieces {
        fst.insert(p, *id)?;
    }

    std::fs::write(dst.join("vocab.fst"), fst.into_inner()?)?;
    std::fs::write(dst.join("scores.f32"), scores)?;
    std::fs::write(dst.join("table.bin"), &table)?;
    std::fs::write(dst.join("scales.f32"), scales)?;
    let _ = std::fs::remove_file(dst.join("table.f16")); // the format before table.bin
    let meta = serde_json::json!({
        "rows": keep.len(),
        "dim": dim,
        "dtype": opts.dtype.name(),
        "unk_id": unk,
        "source": src.file_name().and_then(|n| n.to_str()).unwrap_or(""),
        "keep": if opts.latin_only { "latin" } else { "all" },
        "top": opts.top,
    });
    std::fs::write(dst.join("meta.json"), serde_json::to_vec_pretty(&meta)?)?;
    eprintln!(
        "kept {} of {} pieces, dim {dim}, {}: {:.1} MB",
        keep.len(),
        vocab.len(),
        opts.dtype.name(),
        table.len() as f64 / 1e6
    );
    Ok(())
}
