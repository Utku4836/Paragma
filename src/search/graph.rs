//! What a record is connected to, for an agent: the records about the same
//! code, the ones it replaced or that replaced it, the notes linking to it
//! or linked from it, the ones written in the same work, and the closest in
//! meaning. Said in one line after a record (`pg read`) and after the best
//! results (`pg find`), so that one call brings the record's surroundings.

use crate::embed;
use crate::index::Index;
use crate::project::Project;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;

/// Records listed as connected.
pub const SHOWN: usize = 5;
/// Records at least this close in meaning are "about the same".
const SAME_TOPIC: f32 = 0.72;

/// A connected record: its id, title and how it is connected.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub id: String,
    pub title: String,
    pub why: String,
}

struct Row {
    id: String,
    path: String,
    title: String,
    props: Value,
}

fn list(v: &Value, key: &str) -> Vec<String> {
    match &v[key] {
        Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        Value::String(s) if !s.is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// A file named by two records: the same file, or one a folder of the other.
fn same_place(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim_end_matches('/'), b.trim_end_matches('/'));
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

/// The records connected to `id`, the strongest first, at most `max`,
/// without those in `skip`.
pub fn connected(project: &Project, index: &Index, id: &str, max: usize, skip: &[String]) -> Result<Vec<Link>> {
    connected_by(project, index, id, max, skip, true)
}

/// The same, without "about the same" (`meaning` false): what a search's
/// results, already close in meaning, do not say.
pub fn connected_by(
    project: &Project,
    index: &Index,
    id: &str,
    max: usize,
    skip: &[String],
    meaning: bool,
) -> Result<Vec<Link>> {
    crate::output::mark("connected: start");
    // a note without an id is known by its path, as search names it
    let mut st = index.db.prepare_cached("SELECT id, path, title, props FROM notes")?;
    let rows: Vec<Row> = st
        .query_map([], |r| {
            let props: String = r.get(3)?;
            let path: String = r.get(1)?;
            let id = r.get::<_, Option<String>>(0)?.filter(|s| !s.is_empty());
            Ok(Row {
                id: id.unwrap_or_else(|| path.trim_end_matches(".md").to_string()),
                path,
                title: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                props: serde_json::from_str(&props).unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    // looked up by id and by path many times below: a large memory has
    // thousands of notes
    let ids: HashMap<&str, &Row> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
    let at: HashMap<&str, &Row> = rows.iter().map(|r| (r.path.as_str(), r)).collect();
    let Some(me) = ids.get(id).copied() else { return Ok(Vec::new()) };
    let mut out: Vec<Link> = Vec::new();
    let add = |out: &mut Vec<Link>, other: &Row, why: String| {
        if other.id != id && !skip.contains(&other.id) && !out.iter().any(|l| l.id == other.id) {
            out.push(Link { id: other.id.clone(), title: other.title.clone(), why });
        }
    };
    let by_id = |x: &str| ids.get(x).copied();

    // the decisions it replaced, or that replaced it
    for (key, why) in [("superseded_by", "replaced this"), ("supersedes", "replaced by this")] {
        for other in list(&me.props, key) {
            if let Some(r) = by_id(&other) {
                add(&mut out, r, why.to_string());
            }
        }
    }
    // the same code
    let (files, symbols) = (list(&me.props, "files"), list(&me.props, "symbols"));
    if !files.is_empty() || !symbols.is_empty() {
        for r in &rows {
            let shared = list(&r.props, "files").into_iter().find(|f| files.iter().any(|m| same_place(m, f)));
            let shared = shared.or_else(|| list(&r.props, "symbols").into_iter().find(|s| symbols.contains(s)));
            if let Some(what) = shared {
                add(&mut out, r, format!("same code ({what})"));
            }
        }
    }
    // errors linked as related
    for other in list(&me.props, "related") {
        if let Some(r) = by_id(&other) {
            add(&mut out, r, "a related error".to_string());
        }
    }
    // [[links]] both ways: [[name]], [[folder/name]], [[name#Heading]],
    // [[name|label]], each to the one note it names
    crate::output::mark("connected: notes read");
    let paths: Vec<&str> = rows.iter().map(|r| r.path.as_str()).collect();
    let text = std::fs::read_to_string(project.dir.join(&me.path)).unwrap_or_default();
    for target in wiki_links(&text) {
        if let Some(p) = resolve(&target, &me.path, &paths)
            && let Some(r) = at.get(p.as_str())
        {
            add(&mut out, r, "linked from this".to_string());
        }
    }
    let mut st = index.db.prepare_cached("SELECT DISTINCT path, text FROM chunks WHERE text LIKE '%[[%'")?;
    let linking: Vec<(String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.flatten().collect();
    for (path, text) in linking {
        if path != me.path
            && wiki_links(&text).iter().any(|t| resolve(t, &path, &paths).as_deref() == Some(me.path.as_str()))
            && let Some(r) = at.get(path.as_str())
        {
            add(&mut out, r, "links here".to_string());
        }
    }
    crate::output::mark("connected: links");
    // written in the same work (this computer's history)
    if let Ok(state) = crate::state::State::open(project) {
        for x in state.written_with(id).unwrap_or_default() {
            if let Some(r) = by_id(&x) {
                add(&mut out, r, "written in the same work".to_string());
            }
        }
    }
    if !meaning {
        out.truncate(max);
        return Ok(out);
    }
    crate::output::mark("connected: same work");
    // the closest in meaning
    let mut st = index.db.prepare_cached("SELECT ref, vec FROM chunks")?;
    let pieces: Vec<(String, Vec<f32>)> = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, embed::from_bytes(&r.get::<_, Vec<u8>>(1)?))))?
        .flatten()
        .collect();
    let mine: Vec<&Vec<f32>> = pieces.iter().filter(|p| p.0 == id).map(|p| &p.1).collect();
    let mut best: HashMap<&str, f32> = HashMap::new();
    for (r, v) in pieces.iter().filter(|p| p.0 != id) {
        let s = mine.iter().map(|m| embed::similarity(m, v)).fold(f32::MIN, f32::max);
        if s >= SAME_TOPIC {
            let b = best.entry(r.as_str()).or_insert(s);
            *b = b.max(s);
        }
    }
    crate::output::mark("connected: meaning");
    let mut near: Vec<(String, f32)> = best.into_iter().map(|(r, s)| (r.to_string(), s)).collect();
    // ties in a fixed order: the same note shows the same links each time
    near.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    // only as many as can be shown are looked at
    near.truncate(max + skip.len() + out.len() + 1);
    for (x, _) in near {
        if let Some(r) = by_id(&x) {
            add(&mut out, r, "about the same".to_string());
        } else if !skip.contains(&x) && !out.iter().any(|l| l.id == x) && x != id {
            // a rule: its id is the piece's
            out.push(Link { id: x, title: String::new(), why: "about the same".to_string() });
        }
    }
    out.truncate(max);
    Ok(out)
}

/// The targets of the [[links]] in a text, as written (no label, no
/// heading): "folder/name", "name".
pub fn wiki_links(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("[[") {
        let after = &rest[i + 2..];
        let Some(j) = after.find("]]") else { break };
        let inner = &after[..j];
        let target = inner.split('|').next().unwrap_or("").split(['#', '^']).next().unwrap_or("").trim();
        if !target.is_empty() && !target.contains('\n') && !out.iter().any(|t: &String| t == target) {
            out.push(target.to_string());
        }
        rest = &after[j + 2..];
    }
    out
}

/// The note a link names, from the note at `from`: the note at that path
/// (from the project's root, or the end of a path), else the one note with
/// that name, else of the notes with that name the one nearest to `from`
/// (then the one nearest the root). None when it names none, or several
/// equally near.
pub fn resolve(target: &str, from: &str, paths: &[&str]) -> Option<String> {
    let t = target.trim_end_matches(".md").trim_start_matches("./").to_lowercase();
    let stem = |p: &str| p.trim_end_matches(".md").to_lowercase();
    if t.contains('/') {
        let hits: Vec<&&str> = paths.iter().filter(|p| stem(p) == t || stem(p).ends_with(&format!("/{t}"))).collect();
        return (hits.len() == 1).then(|| hits[0].to_string());
    }
    let named: Vec<&&str> = paths.iter().filter(|p| stem(p).rsplit('/').next() == Some(t.as_str())).collect();
    match named.len() {
        0 => None,
        1 => Some(named[0].to_string()),
        _ => {
            // the nearest: the most folders in common with the linking note
            let from_dirs: Vec<&str> = from.split('/').collect();
            let common = |p: &str| p.split('/').zip(&from_dirs).take_while(|(a, b)| a == *b).count();
            let best = named.iter().map(|p| common(p)).max()?;
            let nearest: Vec<&&&str> = named.iter().filter(|p| common(p) == best).collect();
            // then the one nearest the root, as Obsidian takes it
            let depth = |p: &str| p.matches('/').count();
            let top = nearest.iter().map(|p| depth(p)).min()?;
            let shallow: Vec<&&&&str> = nearest.iter().filter(|p| depth(p) == top).collect();
            (shallow.len() == 1).then(|| shallow[0].to_string())
        }
    }
}

/// "connected: d7 Refund window: replaced this · e4 Day 30 refused: same code (payments/)"
pub fn line(links: &[Link]) -> Option<String> {
    if links.is_empty() {
        return None;
    }
    let parts: Vec<String> = links
        .iter()
        .map(|l| {
            if l.title.is_empty() {
                format!("{} ({})", l.id, l.why)
            } else {
                format!("{} {} ({})", l.id, crate::output::one_line(&l.title, 40), l.why)
            }
        })
        .collect();
    Some(format!("connected: {}", parts.join(" · ")))
}

#[cfg(all(test, paragma_local))]
#[path = "../../unit/src/search/graph.rs"]
mod tests;
