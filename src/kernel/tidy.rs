//! The tidying expert: what in the notes could be kept better, each with the
//! command that does it. pg only says; the agent decides and does. No
//! limits of size: what is found is what the notes mean, by their vectors.
//!
//! - two notes that say nearly the same: read both, keep one;
//! - a note whose parts are about unrelated things: one note per topic;
//! - a folder holding one note: the note where its neighbours are.

/// A note as the expert sees it: its pieces (by heading) with their
/// meaning vectors.
#[derive(Clone, Debug)]
pub struct NoteFacts {
    pub id: String,
    /// Path in the project, without ".md".
    pub path: String,
    pub pieces: Vec<(String, Vec<f32>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tidy {
    /// Two notes nearly the same.
    Same { a: String, b: String },
    /// A note about several unrelated topics: its headings.
    Topics { id: String, headings: Vec<String> },
    /// A folder with only this note in it.
    Alone { folder: String, id: String, path: String },
}

/// Two notes this close in meaning say nearly the same.
pub const SAME: f32 = 0.93;
/// A note's parts this far apart on average are about different things.
/// Measured: sections on unrelated things (a database, fonts, shipping)
/// 0.13; sections that are sides of one topic in real notes (how it works,
/// what was removed, measurements, what is left) 0.32 and above.
pub const APART: f32 = 0.25;
/// Parts needed before a note can be about several topics.
pub const PARTS: usize = 3;

impl Tidy {
    /// What to do, with the commands.
    pub fn text(&self) -> String {
        match self {
            Tidy::Same { a, b } => format!(
                "{a} and {b} are about the same thing: pg read {a}, pg read {b}; one note (pg rm the other), or titles that say how they differ"
            ),
            Tidy::Topics { id, headings } => format!(
                "{id} holds {} topics ({}): one note each, pg write <folder>/<topic> \"...\", then pg rm {id}",
                headings.len(),
                headings.join(", ")
            ),
            Tidy::Alone { folder, id, path } => {
                let parent = folder.rsplit_once('/').map(|(p, _)| format!("{p}/")).unwrap_or_default();
                let name = path.rsplit('/').next().unwrap_or(path);
                format!(
                    "{folder}/ holds one note ({id}): a folder is a topic, each of its subjects its own note there (pg write {folder}/<subject> \"...\"); not a topic: pg mv {path} {parent}{name}"
                )
            }
        }
    }
}

fn mean(vectors: &[&Vec<f32>]) -> Vec<f32> {
    let Some(first) = vectors.first() else { return Vec::new() };
    let mut m = vec![0.0f32; first.len()];
    for v in vectors {
        for (x, y) in m.iter_mut().zip(v.iter()) {
            *x += y;
        }
    }
    let norm = m.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        m.iter_mut().for_each(|x| *x /= norm);
    }
    m
}

fn similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    crate::embed::similarity(a, b)
}

/// How alike a note's headed parts are, on average (None: fewer than
/// `PARTS`): low, they are about different things.
pub fn spread(pieces: &[(String, Vec<f32>)]) -> Option<f32> {
    let parts: Vec<&(String, Vec<f32>)> = pieces.iter().filter(|p| !p.0.is_empty()).collect();
    if parts.len() < PARTS {
        return None;
    }
    let mut sum = 0.0;
    let mut pairs = 0;
    for i in 0..parts.len() {
        for j in i + 1..parts.len() {
            sum += similarity(&parts[i].1, &parts[j].1);
            pairs += 1;
        }
    }
    (pairs > 0).then(|| sum / pairs as f32)
}

pub fn tidy(notes: &[NoteFacts]) -> Vec<Tidy> {
    let mut out = Vec::new();
    // nearly the same: the whole of each note, compared
    let whole: Vec<Vec<f32>> = notes.iter().map(|n| mean(&n.pieces.iter().map(|p| &p.1).collect::<Vec<_>>())).collect();
    for i in 0..notes.len() {
        for j in i + 1..notes.len() {
            if similarity(&whole[i], &whole[j]) >= SAME {
                out.push(Tidy::Same { a: notes[i].id.clone(), b: notes[j].id.clone() });
            }
        }
    }
    // several topics: parts that are, on average, about different things
    for n in notes {
        let parts: Vec<&(String, Vec<f32>)> = n.pieces.iter().filter(|p| !p.0.is_empty()).collect();
        if spread(&n.pieces).is_some_and(|s| s < APART) {
            let mut headings: Vec<String> = parts.iter().map(|p| p.0.clone()).collect();
            headings.dedup();
            out.push(Tidy::Topics { id: n.id.clone(), headings });
        }
    }
    // a folder with one note
    let mut folders: std::collections::BTreeMap<&str, Vec<&NoteFacts>> = std::collections::BTreeMap::new();
    for n in notes {
        if let Some((folder, _)) = n.path.rsplit_once('/') {
            folders.entry(folder).or_default().push(n);
        }
    }
    // a folder that holds folders is not alone
    let parents: Vec<&str> = folders.keys().filter_map(|f| f.rsplit_once('/').map(|(p, _)| p)).collect();
    for (folder, list) in &folders {
        if let [n] = list.as_slice()
            && !parents.contains(folder)
        {
            out.push(Tidy::Alone { folder: folder.to_string(), id: n.id.clone(), path: n.path.clone() });
        }
    }
    out
}

#[cfg(all(test, paragma_local))]
#[path = "../../unit/src/kernel/tidy.rs"]
mod tests;
