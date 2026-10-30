//! Keeping the index equal to the note files.
//!
//! Every command compares the directory listing with the index (times and
//! sizes); only notes that changed outside pg (an editor, Obsidian) are
//! read again. pg's own writes update the index at once (`Index::refresh`).
//! Reading notes and computing their vectors runs on every core; only the
//! inserts are serial.

use super::Index;
use crate::embed;
use crate::note::{self, Note};
use crate::project::{Project, files, scan};
use anyhow::Result;
use rusqlite::{Transaction, params};
use std::collections::HashMap;

impl Index {
    pub fn sync(&mut self, project: &Project) -> Result<()> {
        let disk = scan::notes(&project.dir);
        if changes_in(&self.db, &disk)?.is_empty() {
            return Ok(()); // the common case: nothing to write, no lock taken
        }
        // several pg may start together: the first to get the write lock does
        // the work, the others find it done when they look again
        let tx = self.db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut to_read = Vec::new();
        for change in changes_in(&tx, &disk)? {
            match change {
                Change::Gone(rel) => remove_file(&tx, &rel)?,
                Change::New(rel) | Change::Edited(rel) => to_read.push(rel),
            }
        }
        for prepared in prepare_all(project, &to_read) {
            remove_file(&tx, &prepared.rel)?;
            store(&tx, &prepared)?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[derive(Debug)]
enum Change {
    New(String),
    Edited(String),
    Gone(String),
}

fn changes_in(db: &rusqlite::Connection, disk: &[scan::FileState]) -> Result<Vec<Change>> {
    let mut known: HashMap<String, (i64, u64)> = HashMap::new();
    let mut st = db.prepare("SELECT path, mtime, size FROM notes")?;
    for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))? {
        let (p, m, s) = r?;
        known.insert(p, (m, s as u64));
    }
    let mut out = Vec::new();
    for f in disk {
        match known.remove(&f.rel) {
            None => out.push(Change::New(f.rel.clone())),
            Some((m, s)) if m != f.mtime || s != f.size => out.push(Change::Edited(f.rel.clone())),
            Some(_) => {}
        }
    }
    out.extend(known.into_keys().map(Change::Gone));
    Ok(out)
}

fn remove_file(tx: &Transaction, rel: &str) -> Result<()> {
    tx.execute("DELETE FROM chunks_fts WHERE rowid IN (SELECT rowid FROM chunks WHERE path = ?1)", [rel])?;
    tx.execute("DELETE FROM chunks WHERE path = ?1", [rel])?;
    tx.execute("DELETE FROM notes WHERE path = ?1", [rel])?;
    Ok(())
}

/// Reads one note file and replaces what the index holds about it.
pub(super) fn reindex_file(tx: &Transaction, project: &Project, rel: &str) -> Result<()> {
    remove_file(tx, rel)?;
    if let Some(p) = prepare(project, rel) {
        store(tx, &p)?;
    }
    Ok(())
}

/// A note read and cut into pieces with their vectors, ready to insert.
struct Prepared {
    rel: String,
    id: Option<String>,
    kind: &'static str,
    title: String,
    status: String,
    mtime: i64,
    size: u64,
    props: String,
    pieces: Vec<(note::Chunk, Vec<f32>)>,
}

/// Prepares many notes at once, one share of them per core.
fn prepare_all(project: &Project, rels: &[String]) -> Vec<Prepared> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
    if rels.len() < 8 || threads == 1 {
        return rels.iter().filter_map(|r| prepare(project, r)).collect();
    }
    let size = rels.len().div_ceil(threads);
    std::thread::scope(|s| {
        let handles: Vec<_> = rels
            .chunks(size)
            .map(|part| s.spawn(move || part.iter().filter_map(|r| prepare(project, r)).collect::<Vec<_>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    })
}

fn prepare(project: &Project, rel: &str) -> Option<Prepared> {
    let path = project.dir.join(rel);
    let meta = std::fs::metadata(&path).ok()?;
    let text = files::read_text(&path).ok()?;
    let n = Note::parse(&text);
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    let fallback_title = rel.trim_end_matches(".md").rsplit('/').next().unwrap_or(rel).to_string();
    let title = Some(n.title()).filter(|t| !t.is_empty()).unwrap_or(fallback_title);
    let props: serde_json::Map<String, serde_json::Value> = n
        .props
        .iter()
        .map(|(k, v)| {
            let v = match v {
                note::Value::Text(s) => serde_json::Value::String(s.clone()),
                note::Value::List(l) => serde_json::Value::from(l.clone()),
            };
            (k.clone(), v)
        })
        .collect();
    let enc = embed::encoder();
    let pieces = note::chunks(&n, rel.trim_end_matches(".md"))
        .into_iter()
        .map(|c| {
            let v = enc.encode(&format!("{title}. {}. {}", c.heading, c.text));
            (c, v)
        })
        .collect();
    Some(Prepared {
        rel: rel.to_string(),
        id: n.id().map(str::to_string),
        kind: n.kind().as_str(),
        title,
        status: n.props.text("status").unwrap_or("active").to_string(),
        mtime,
        size: meta.len(),
        props: serde_json::Value::Object(props).to_string(),
        pieces,
    })
}

fn store(tx: &Transaction, p: &Prepared) -> Result<()> {
    tx.execute(
        "INSERT INTO notes(path, id, kind, title, status, mtime, size, props) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![p.rel, p.id, p.kind, p.title, p.status, p.mtime, p.size as i64, p.props],
    )?;
    let mut insert = tx.prepare_cached(
        "INSERT INTO chunks(path, ref, kind, title, heading, text, vec) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    let mut insert_fts =
        tx.prepare_cached("INSERT INTO chunks_fts(rowid, title, heading, text) VALUES (?1, ?2, ?3, ?4)")?;
    for (c, v) in &p.pieces {
        insert.execute(params![p.rel, c.id, c.kind.as_str(), p.title, c.heading, c.text, embed::to_bytes(v)])?;
        insert_fts.execute(params![tx.last_insert_rowid(), p.title, c.heading, c.text])?;
    }
    Ok(())
}
