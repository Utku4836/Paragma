//! Builds the embedding model that pg embeds in its binary.
//!
//!   build-model <model2vec-folder> <out-folder>
//!
//! The input is a Model2Vec model with a Unigram tokenizer (we use
//! minishlab/potion-multilingual-128M; `fetch.sh` downloads it). The output
//! keeps only the Latin-script pieces (English, Turkish and the other
//! Latin-alphabet languages) and stores them so pg loads in a few ms:
//!
//!   vocab.fst    piece -> id (a finite state transducer, read in place)
//!   scores.f32   Unigram score per id
//!   table.f16    one row of `dim` f16 values per id
//!   meta.json    rows, dim, unk_id, source

use anyhow::{Context, Result, bail};
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [src, dst] = args.as_slice() else {
        bail!("usage: build-model <model2vec-folder> <out-folder>");
    };
    build(Path::new(src), Path::new(dst))
}

fn is_latin(c: char) -> bool {
    c == '\u{2581}' || (c.is_ascii() && !c.is_ascii_control()) || ('\u{00C0}'..='\u{017F}').contains(&c)
}

fn build(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    let tokenizer: serde_json::Value = serde_json::from_slice(&std::fs::read(src.join("tokenizer.json"))?)?;
    if tokenizer["model"]["type"] != "Unigram" {
        bail!("only Unigram tokenizers are supported");
    }
    let vocab = tokenizer["model"]["vocab"].as_array().context("tokenizer has no vocab")?;
    let unk = tokenizer["model"]["unk_id"].as_u64().context("tokenizer has no unk_id")? as usize;

    // pieces kept: the special tokens and every Latin-script piece
    let keep: Vec<usize> = (0..vocab.len())
        .filter(|&i| i <= unk || vocab[i][0].as_str().is_some_and(|p| p.chars().all(is_latin)))
        .collect();

    let bytes = std::fs::read(src.join("model.safetensors"))?;
    let tensors = safetensors::SafeTensors::deserialize(&bytes)?;
    let embeddings = tensors.tensor("embeddings")?;
    let dim = embeddings.shape()[1];
    let raw = embeddings.data();
    let value = |i: usize| -> f32 {
        match embeddings.dtype() {
            safetensors::Dtype::F16 => half::f16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]).to_f32(),
            _ => f32::from_le_bytes([raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]]),
        }
    };

    let mut table = Vec::with_capacity(keep.len() * dim * 2);
    let mut scores = Vec::with_capacity(keep.len() * 4);
    let mut pieces: Vec<(String, u64)> = Vec::with_capacity(keep.len());
    for (new_id, &old_id) in keep.iter().enumerate() {
        for j in 0..dim {
            table.extend_from_slice(&half::f16::from_f32(value(old_id * dim + j)).to_le_bytes());
        }
        scores.extend_from_slice(&(vocab[old_id][1].as_f64().unwrap_or(0.0) as f32).to_le_bytes());
        pieces.push((vocab[old_id][0].as_str().unwrap_or("").to_string(), new_id as u64));
    }
    pieces.sort();
    pieces.dedup_by(|a, b| a.0 == b.0);
    let mut fst = fst::MapBuilder::memory();
    for (piece, id) in &pieces {
        fst.insert(piece, *id)?;
    }

    std::fs::write(dst.join("vocab.fst"), fst.into_inner()?)?;
    std::fs::write(dst.join("scores.f32"), scores)?;
    std::fs::write(dst.join("table.f16"), table)?;
    let meta = serde_json::json!({
        "rows": keep.len(),
        "dim": dim,
        "unk_id": unk,
        "source": src.file_name().and_then(|n| n.to_str()).unwrap_or(""),
    });
    std::fs::write(dst.join("meta.json"), serde_json::to_vec_pretty(&meta)?)?;
    eprintln!("kept {} of {} pieces, dim {dim}", keep.len(), vocab.len());
    Ok(())
}
