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
    let mut st =
        index.db.prepare_cached("SELECT id, path, title, props FROM notes WHERE id IS NOT NULL AND id != ''")?;
    let rows: Vec<Row> = st
        .query_map([], |r| {
            let props: String = r.get(3)?;
            Ok(Row {
                id: r.get(0)?,
                path: r.get(1)?,
                title: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                props: serde_json::from_str(&props).unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let Some(me) = rows.iter().find(|r| r.id == id) else { return Ok(Vec::new()) };
    let mut out: Vec<Link> = Vec::new();
    let add = |out: &mut Vec<Link>, other: &Row, why: String| {
        if other.id != id && !skip.contains(&other.id) && !out.iter().any(|l| l.id == other.id) {
            out.push(Link { id: other.id.clone(), title: other.title.clone(), why });
        }
    };
    let by_id = |x: &str| rows.iter().find(|r| r.id == x);

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
    // [[links]] both ways, by the note's name
    let name = |path: &str| path.trim_end_matches(".md").rsplit('/').next().unwrap_or("").to_string();
    let my_name = name(&me.path);
    let text = std::fs::read_to_string(project.dir.join(&me.path)).unwrap_or_default();
    for r in &rows {
        let theirs = name(&r.path);
        if !theirs.is_empty() && (text.contains(&format!("[[{theirs}]]")) || text.contains(&format!("[[{theirs}|"))) {
            add(&mut out, r, "linked from this".to_string());
        }
    }
    if !my_name.is_empty() {
        let mut st = index.db.prepare_cached("SELECT DISTINCT ref FROM chunks WHERE text LIKE ?1 OR text LIKE ?2")?;
        let refs: Vec<String> =
            st.query_map([format!("%[[{my_name}]]%"), format!("%[[{my_name}|%")], |r| r.get(0))?.flatten().collect();
        for x in refs {
            if let Some(r) = by_id(&x) {
                add(&mut out, r, "links here".to_string());
            }
        }
    }
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
    // the closest in meaning
    let mut st = index.db.prepare_cached("SELECT ref, vec FROM chunks")?;
    let pieces: Vec<(String, Vec<f32>)> = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, embed::from_bytes(&r.get::<_, Vec<u8>>(1)?))))?
        .flatten()
        .collect();
    let mine: Vec<&Vec<f32>> = pieces.iter().filter(|p| p.0 == id).map(|p| &p.1).collect();
    let mut near: Vec<(String, f32)> = Vec::new();
    for (r, v) in pieces.iter().filter(|p| p.0 != id) {
        let s = mine.iter().map(|m| embed::similarity(m, v)).fold(f32::MIN, f32::max);
        if s >= SAME_TOPIC {
            match near.iter_mut().find(|n| n.0 == *r) {
                Some(n) => n.1 = n.1.max(s),
                None => near.push((r.clone(), s)),
            }
        }
    }
    near.sort_by(|a, b| b.1.total_cmp(&a.1));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_and_a_file_in_it_are_the_same_place() {
        assert!(same_place("payments/", "payments/refund.go"));
        assert!(same_place("payments/refund.go", "payments/refund.go"));
        assert!(!same_place("pay", "payments/refund.go"));
    }
}
