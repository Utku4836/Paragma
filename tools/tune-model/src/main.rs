//! Fine-tunes the Model2Vec table so a sentence and its translation get
//! close vectors. Same pieces, same size: only the values of the rows move.
//!
//!   tune-model <model2vec-folder> <tatoeba-folder> <out-folder> [--pairs N] [--epochs N] [--lr X] [--temp X] [--hold X] [--mix X]
//!
//! Multilingual knowledge distillation (Reimers & Gurevych 2020): the
//! original model's vector of an English sentence is the target for the
//! sentence itself and for its translation, so English stays where it was
//! and the other languages move onto it. With --temp, a translation is also
//! told apart from the other English sentences of its batch (InfoNCE at that
//! temperature). --hold pulls every changed row back toward the original by
//! that fraction per step, so pieces the sentences rarely show do not drift.
//! --mix keeps that part of the change and no more (original + mix x change;
//! WiSE-FT, Wortsman et al. 2022): a half-way model keeps short English
//! queries as good as the original while the other languages still gain.
//!
//! The pairs come from Tatoeba (tatoeba.org, CC BY 2.0 FR), per language xxx:
//! downloads.tatoeba.org/exports/per_language/xxx/xxx_sentences.tsv and
//! xxx-eng_links.tsv, with eng/eng_sentences.tsv. The output is a Model2Vec
//! folder that build-model takes, with tuning.json recording how it was made.
//!
//! Each epoch prints, per language, how often a held-out translation finds
//! its English sentence first among 1000, and how close English stayed to
//! the original model.

use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// pg's tokenizer, so training sees the pieces pg will.
#[path = "../../../src/embed/tokenize.rs"]
mod tokenize;

/// The token weighting pg uses (src/embed/mod.rs).
const SIF_A: f32 = 0.001;
const LANGS: [&str; 6] = ["tur", "spa", "deu", "fra", "por", "ita"];
/// Pairs per language kept out of training to measure with.
const HELD_OUT: usize = 1000;
const BATCH: usize = 256;

/// What the tokenizer needs: piece lookup, the unknown id, the lowest score.
pub struct Encoder {
    pieces: HashMap<String, (u32, f32)>,
    unk: u32,
    min_score: f32,
}

impl Encoder {
    /// The pieces the text starts with: (length in characters, id, score).
    fn prefixes(&self, chars: &[char], out: &mut Vec<(usize, u32, f32)>) {
        out.clear();
        let mut piece = String::new();
        for (k, c) in chars.iter().enumerate() {
            piece.push(*c);
            if let Some(&(id, score)) = self.pieces.get(&piece) {
                out.push((k + 1, id, score));
            }
        }
    }
}

struct Options {
    pairs: usize,
    epochs: usize,
    lr: f32,
    temp: f32,
    hold: f32,
    mix: f32,
}

/// A sentence as token ids with their weights.
type Tokens = Vec<(u32, f32)>;

struct Pair {
    en: Tokens,
    other: Tokens,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: tune-model <model2vec-folder> <tatoeba-folder> <out-folder> [--pairs N] [--epochs N] [--lr X] [--temp X] [--hold X] [--mix X]";
    if args.len() < 3 || args[..3].iter().any(|a| a.starts_with("--")) {
        bail!(usage);
    }
    let number = |name: &str, default: f32| -> Result<f32> {
        match args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)) {
            Some(v) => v.parse().with_context(|| format!("{name} takes a number")),
            None => Ok(default),
        }
    };
    let opts = Options {
        pairs: number("--pairs", 200_000.0)? as usize,
        epochs: number("--epochs", 3.0)? as usize,
        lr: number("--lr", 0.001)?,
        temp: number("--temp", 0.0)?,
        hold: number("--hold", 0.0)?,
        mix: number("--mix", 1.0)?,
    };
    let (src, data, dst) = (Path::new(&args[0]), Path::new(&args[1]), Path::new(&args[2]));

    let (enc, weight) = load_tokenizer(src)?;
    let (orig, dim) = load_table(src)?;
    let (train, held) = load_pairs(data, &enc, &weight, opts.pairs)?;
    eprintln!("{} training pairs, {} held out per language", train.len(), HELD_OUT);

    let mut table = orig.clone();
    let mut log = vec![measure(&table, &orig, dim, &held)];
    eprintln!("before: {}", log[0]);
    let mut adam = Adam::default();
    let mut order: Vec<usize> = (0..train.len()).collect();
    let mut rng = 0x5eed_u64;
    for epoch in 1..=opts.epochs {
        shuffle(&mut order, &mut rng);
        let (mut loss, mut drift) = (0.0, 0.0);
        for batch in order.chunks(BATCH) {
            let pairs: Vec<&Pair> = batch.iter().map(|&i| &train[i]).collect();
            let (l, d) = step(&mut table, &orig, dim, &pairs, &mut adam, &opts);
            loss += l;
            drift += d;
        }
        let n = train.len() as f64;
        let line = format!(
            "epoch {epoch}: loss {:.4}, English drift {:.4}; {}",
            loss / n,
            drift / n,
            measure(&table, &orig, dim, &held)
        );
        eprintln!("{line}");
        log.push(line);
    }
    if opts.mix < 1.0 {
        table.iter_mut().zip(&orig).for_each(|(t, o)| *t = o + opts.mix * (*t - o));
        let line = format!("mixed {}: {}", opts.mix, measure(&table, &orig, dim, &held));
        eprintln!("{line}");
        log.push(line);
    }

    std::fs::create_dir_all(dst)?;
    for f in ["tokenizer.json", "config.json", "modules.json"] {
        if src.join(f).exists() {
            std::fs::copy(src.join(f), dst.join(f))?;
        }
    }
    let bytes: Vec<u8> = table.iter().flat_map(|x| x.to_le_bytes()).collect();
    let view = safetensors::tensor::TensorView::new(safetensors::Dtype::F32, vec![table.len() / dim, dim], &bytes)?;
    safetensors::serialize_to_file([("embeddings", view)], None, &dst.join("model.safetensors"))?;
    let record = serde_json::json!({
        "source": src.file_name().and_then(|n| n.to_str()),
        "data": "Tatoeba (tatoeba.org), CC BY 2.0 FR",
        "languages": LANGS,
        "pairs_per_language": opts.pairs,
        "epochs": opts.epochs,
        "lr": opts.lr,
        "temp": opts.temp,
        "hold": opts.hold,
        "mix": opts.mix,
        "log": log,
    });
    std::fs::write(dst.join("tuning.json"), serde_json::to_vec_pretty(&record)?)?;
    Ok(())
}

/// The tokenizer over the whole vocabulary, and each piece's SIF weight.
fn load_tokenizer(src: &Path) -> Result<(Encoder, Vec<f32>)> {
    let tokenizer: serde_json::Value = serde_json::from_slice(&std::fs::read(src.join("tokenizer.json"))?)?;
    let vocab = tokenizer["model"]["vocab"].as_array().context("tokenizer has no vocab")?;
    let unk = tokenizer["model"]["unk_id"].as_u64().context("tokenizer has no unk_id")? as u32;
    let mut pieces = HashMap::with_capacity(vocab.len());
    let mut weight = Vec::with_capacity(vocab.len());
    let mut min_score = f32::MAX;
    for (i, v) in vocab.iter().enumerate() {
        let score = v[1].as_f64().unwrap_or(f64::MIN) as f32;
        pieces.entry(v[0].as_str().unwrap_or("").to_string()).or_insert((i as u32, score));
        weight.push(SIF_A / (SIF_A + score.exp()));
        if i as u32 != unk {
            min_score = min_score.min(score);
        }
    }
    Ok((Encoder { pieces, unk, min_score }, weight))
}

/// The embedding table as f32, and its width.
fn load_table(src: &Path) -> Result<(Vec<f32>, usize)> {
    let bytes = std::fs::read(src.join("model.safetensors"))?;
    let tensors = safetensors::SafeTensors::deserialize(&bytes)?;
    let emb = tensors.tensor("embeddings")?;
    let raw = emb.data();
    let table = match emb.dtype() {
        safetensors::Dtype::F32 => raw.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect(),
        safetensors::Dtype::F16 => {
            raw.as_chunks::<2>().0.iter().map(|b| half::f16::from_le_bytes(*b).to_f32()).collect()
        }
        d => bail!("unsupported dtype {d:?}"),
    };
    Ok((table, emb.shape()[1]))
}

/// Shuffled pairs: training ones (at most `cap` per language), and per
/// language the held-out ones, whose English sentences training never sees.
fn load_pairs(data: &Path, enc: &Encoder, weight: &[f32], cap: usize) -> Result<(Vec<Pair>, Vec<Vec<Pair>>)> {
    let english = sentences(&data.join("eng_sentences.tsv"))?;
    let mut rng = 0x7a70eba_u64;
    let mut lists = Vec::new();
    for lang in LANGS {
        let other = sentences(&data.join(format!("{lang}_sentences.tsv")))?;
        let links = std::fs::read_to_string(data.join(format!("{lang}-eng_links.tsv")))?;
        let mut list: Vec<(String, String)> = links
            .lines()
            .filter_map(|l| {
                let (a, b) = l.split_once('\t')?;
                let (a, b): (u64, u64) = (a.parse().ok()?, b.parse().ok()?);
                Some((english.get(&b)?.clone(), other.get(&a)?.clone()))
            })
            .collect();
        shuffle(&mut list, &mut rng);
        lists.push(list);
    }
    let held_en: HashSet<&String> = lists.iter().flat_map(|l| l.iter().take(HELD_OUT).map(|p| &p.0)).collect();
    let mut train_text = Vec::new();
    let mut held_text = Vec::new();
    for list in &lists {
        held_text.push(list[..HELD_OUT.min(list.len())].to_vec());
        train_text
            .extend(list[HELD_OUT.min(list.len())..].iter().filter(|p| !held_en.contains(&p.0)).take(cap).cloned());
    }
    let train = tokenize_pairs(enc, weight, &train_text);
    let held = held_text.iter().map(|h| tokenize_pairs(enc, weight, h)).collect();
    Ok((train, held))
}

/// id -> text of a Tatoeba sentences file (id, language, text).
fn sentences(path: &Path) -> Result<HashMap<u64, String>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text
        .lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            let id = f.next()?.parse().ok()?;
            f.next()?;
            Some((id, f.next()?.to_string()))
        })
        .collect())
}

fn tokenize_pairs(enc: &Encoder, weight: &[f32], pairs: &[(String, String)]) -> Vec<Pair> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let chunk = pairs.len().div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let parts: Vec<_> = pairs
            .chunks(chunk)
            .map(|part| {
                s.spawn(move || {
                    part.iter()
                        .map(|(en, other)| Pair { en: tokens(enc, weight, en), other: tokens(enc, weight, other) })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        parts.into_iter().flat_map(|p| p.join().expect("tokenizer thread")).collect()
    })
}

/// A text's tokens with their summed weights, as pg weighs them.
fn tokens(enc: &Encoder, weight: &[f32], text: &str) -> Tokens {
    let mut out: Tokens = Vec::new();
    for id in tokenize::ids(enc, text) {
        if id <= enc.unk {
            continue; // padding and unknown tokens carry no meaning
        }
        match out.iter_mut().find(|t| t.0 == id) {
            Some(t) => t.1 += weight[id as usize],
            None => out.push((id, weight[id as usize])),
        }
    }
    out
}

/// A sentence's unit vector, and its length before normalising.
fn encode(table: &[f32], dim: usize, t: &Tokens) -> (Vec<f32>, f32) {
    let mut v = vec![0f32; dim];
    for &(id, w) in t {
        for (x, r) in v.iter_mut().zip(&table[id as usize * dim..][..dim]) {
            *x += w * r;
        }
    }
    let n = dot(&v, &v).sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    (v, n)
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// One batch: the gradients of the losses on the student's vectors go back
/// through the normalisation to the rows of the sentences' tokens, then one
/// Adam step. Returns the summed distillation loss and English drift.
fn step(table: &mut [f32], orig: &[f32], dim: usize, pairs: &[&Pair], adam: &mut Adam, opts: &Options) -> (f64, f64) {
    let targets: Vec<Vec<f32>> = pairs.iter().map(|p| encode(orig, dim, &p.en).0).collect();
    let mut grads: HashMap<u32, Vec<f32>> = HashMap::new();
    let (mut loss, mut drift) = (0.0, 0.0);
    for (i, p) in pairs.iter().enumerate() {
        let u = &targets[i];
        // the translation: 1 - s.u, so the gradient on s is -u
        let (s, n) = encode(table, dim, &p.other);
        loss += (1.0 - dot(&s, u)) as f64;
        let mut g: Vec<f32> = u.iter().map(|x| -x).collect();
        if opts.temp > 0.0 {
            // -log softmax(s.u_j / temp)[i]: gradient (sum_j p_j u_j - u_i) / temp
            let logits: Vec<f32> = targets.iter().map(|t| dot(&s, t) / opts.temp).collect();
            let max = logits.iter().copied().fold(f32::MIN, f32::max);
            let exps: Vec<f32> = logits.iter().map(|l| (l - max).exp()).collect();
            let sum: f32 = exps.iter().sum();
            for (t, e) in targets.iter().zip(&exps) {
                let pj = e / sum / opts.temp;
                g.iter_mut().zip(t).for_each(|(gx, tx)| *gx += pj * tx);
            }
            g.iter_mut().zip(u).for_each(|(gx, ux)| *gx -= ux / opts.temp);
        }
        backprop(&mut grads, dim, &p.other, &s, n, &g);
        // the English sentence stays where the original model put it
        let (s, n) = encode(table, dim, &p.en);
        drift += (1.0 - dot(&s, u)) as f64;
        let g: Vec<f32> = u.iter().map(|x| -x).collect();
        backprop(&mut grads, dim, &p.en, &s, n, &g);
    }
    adam.update(table, orig, dim, grads, pairs.len() as f32, opts);
    (loss, drift)
}

/// d/dv of a loss with gradient g on s = v / |v| is (g - (g.s) s) / |v|;
/// each token's row gets that times its weight.
fn backprop(grads: &mut HashMap<u32, Vec<f32>>, dim: usize, t: &Tokens, s: &[f32], n: f32, g: &[f32]) {
    if n == 0.0 {
        return;
    }
    let gs = dot(g, s);
    let gv: Vec<f32> = g.iter().zip(s).map(|(gx, sx)| (gx - gs * sx) / n).collect();
    for &(id, w) in t {
        let row = grads.entry(id).or_insert_with(|| vec![0f32; dim]);
        row.iter_mut().zip(&gv).for_each(|(r, x)| *r += w * x);
    }
}

/// Adam that only keeps state for rows a batch touched.
#[derive(Default)]
struct Adam {
    moments: HashMap<u32, (Vec<f32>, Vec<f32>)>,
    steps: i32,
}

impl Adam {
    fn update(
        &mut self,
        table: &mut [f32],
        orig: &[f32],
        dim: usize,
        grads: HashMap<u32, Vec<f32>>,
        batch: f32,
        opts: &Options,
    ) {
        const B1: f32 = 0.9;
        const B2: f32 = 0.999;
        self.steps += 1;
        let (c1, c2) = (1.0 - B1.powi(self.steps), 1.0 - B2.powi(self.steps));
        for (id, g) in grads {
            let (m, v) = self.moments.entry(id).or_insert_with(|| (vec![0f32; dim], vec![0f32; dim]));
            let at = id as usize * dim;
            for k in 0..dim {
                let gk = g[k] / batch;
                m[k] = B1 * m[k] + (1.0 - B1) * gk;
                v[k] = B2 * v[k] + (1.0 - B2) * gk * gk;
                table[at + k] -= opts.lr * (m[k] / c1) / ((v[k] / c2).sqrt() + 1e-8);
                table[at + k] += opts.hold * (orig[at + k] - table[at + k]);
            }
        }
    }
}

/// Per language, how often a held-out translation's nearest English sentence
/// (of the 1000) is its own; and how close English stayed to the original.
fn measure(table: &[f32], orig: &[f32], dim: usize, held: &[Vec<Pair>]) -> String {
    let mut parts = Vec::new();
    let (mut same, mut count) = (0.0, 0);
    let mut total = 0.0;
    for (lang, pairs) in LANGS.iter().zip(held) {
        let en: Vec<Vec<f32>> = pairs.iter().map(|p| encode(table, dim, &p.en).0).collect();
        let hits = pairs
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                let s = encode(table, dim, &p.other).0;
                let best = en.iter().map(|e| dot(&s, e)).enumerate().max_by(|a, b| a.1.total_cmp(&b.1));
                best.map(|b| b.0) == Some(*i)
            })
            .count();
        let acc = hits as f64 / pairs.len().max(1) as f64;
        total += acc;
        parts.push(format!("{lang} {:.1}%", acc * 100.0));
        for (p, e) in pairs.iter().zip(&en) {
            same += dot(e, &encode(orig, dim, &p.en).0) as f64;
            count += 1;
        }
    }
    format!(
        "found {:.1}% ({}), English kept {:.4}",
        total / held.len() as f64 * 100.0,
        parts.join(", "),
        same / count.max(1) as f64
    )
}

fn shuffle<T>(v: &mut [T], rng: &mut u64) {
    for i in (1..v.len()).rev() {
        // splitmix64
        *rng = rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = *rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        v.swap(i, (z % (i as u64 + 1)) as usize);
    }
}
