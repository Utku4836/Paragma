//! The project's search index (`.pg/index.db`): the notes, their pieces
//! with full-text and meaning vectors. Everything in it comes from the
//! note files, so it can be deleted at any time and is rebuilt.

mod edit;
mod sync;

pub use edit::update_note;

use crate::note::Kind;
use crate::project::Project;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

/// Bumped whenever the tables change; an index of another version, or
/// built with another model, is rebuilt.
const SCHEMA_VERSION: &str = "1";

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE notes(
    path TEXT PRIMARY KEY,
    id TEXT,
    kind TEXT,
    title TEXT,
    status TEXT,
    mtime INTEGER,
    size INTEGER,
    props TEXT
);
CREATE INDEX notes_id ON notes(id);
CREATE TABLE chunks(
    rowid INTEGER PRIMARY KEY,
    path TEXT,
    ref TEXT,
    kind TEXT,
    title TEXT,
    heading TEXT,
    text TEXT,
    vec BLOB
);
CREATE INDEX chunks_path ON chunks(path);
CREATE VIRTUAL TABLE chunks_fts USING fts5(title, heading, text, tokenize = 'unicode61 remove_diacritics 2');
";

pub struct Index {
    pub db: Connection,
}

/// A note as the index knows it.
#[derive(Clone, Debug)]
pub struct NoteRow {
    pub path: String,
    pub id: String,
    pub kind: Kind,
}

/// A note's properties as the index keeps them (JSON).
#[derive(Clone, Debug)]
pub struct NoteProps {
    pub id: String,
    pub path: String,
    pub props: serde_json::Value,
    /// When the file last changed, seconds since the epoch.
    pub changed: i64,
}

/// A piece of a note: a whole short note, a section, or a rule.
#[derive(Clone, Debug)]
pub struct Piece {
    pub id: String,
    pub path: String,
    pub text: String,
}

impl Index {
    /// Opens the index, creating or rebuilding it when needed, and brings it
    /// up to date with the note files.
    pub fn open(project: &Project) -> Result<Index> {
        let db = crate::db::open(&project.index_path())?;
        let mut index = Index { db };
        index.ensure_schema()?;
        index.sync(project)?;
        Ok(index)
    }

    fn ensure_schema(&mut self) -> Result<()> {
        let version = |db: &Connection| -> Option<String> {
            db.query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| r.get(0)).ok()
        };
        let wanted = format!("{SCHEMA_VERSION}/{}", crate::embed::fingerprint());
        if version(&self.db).as_deref() == Some(wanted.as_str()) {
            return Ok(());
        }
        // one process builds it; the others wait for the write lock and find it done
        let tx = self.db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if version(&tx).as_deref() != Some(wanted.as_str()) {
            tx.execute_batch(
                "DROP TABLE IF EXISTS meta; DROP TABLE IF EXISTS notes; DROP TABLE IF EXISTS chunks; DROP TABLE IF EXISTS chunks_fts;",
            )?;
            tx.execute_batch(SCHEMA)?;
            tx.execute("INSERT INTO meta(key, value) VALUES ('schema', ?1)", [&wanted])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn notes(&self) -> Result<Vec<NoteRow>> {
        let mut st = self.db.prepare("SELECT path, id, kind FROM notes ORDER BY path")?;
        let rows = st.query_map([], row_to_note)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn note_by_path(&self, path: &str) -> Result<Option<NoteRow>> {
        Ok(self.db.query_row("SELECT path, id, kind FROM notes WHERE path = ?1", [path], row_to_note).optional()?)
    }

    /// The note with this id, or the note holding the rule with this id.
    pub fn note_by_ref(&self, id: &str) -> Result<Option<NoteRow>> {
        let path: Option<String> = self
            .db
            .query_row(
                "SELECT path FROM notes WHERE id = ?1 UNION SELECT path FROM chunks WHERE ref = ?1 LIMIT 1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        match path {
            Some(p) => self.note_by_path(&p),
            None => Ok(None),
        }
    }

    /// Every piece of a kind: what a new record is compared with.
    pub fn pieces_of(&self, kind: Kind) -> Result<Vec<Piece>> {
        let mut st = self.db.prepare("SELECT ref, path, text FROM chunks WHERE kind = ?1")?;
        let rows = st
            .query_map([kind.as_str()], |r| Ok(Piece { id: r.get(0)?, path: r.get(1)?, text: r.get(2)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The error note with this signature.
    pub fn error_with_signature(&self, signature: &str) -> Result<Option<NoteRow>> {
        Ok(self
            .db
            .query_row(
                "SELECT path, id, kind FROM notes WHERE kind = 'error' AND json_extract(props, '$.signature') = ?1",
                [signature],
                row_to_note,
            )
            .optional()?)
    }

    /// Every note of a kind that has an id, with its properties.
    pub fn notes_of(&self, kind: Kind) -> Result<Vec<NoteProps>> {
        let mut st = self.db.prepare("SELECT id, path, props, mtime FROM notes WHERE kind = ?1 AND id IS NOT NULL")?;
        let rows = st
            .query_map([kind.as_str()], |r| {
                Ok(NoteProps {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    props: serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default(),
                    changed: r.get::<_, i64>(3)? / 1_000_000_000,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// A note's properties as JSON (Null when it has none).
    pub fn props_at(&self, path: &str) -> serde_json::Value {
        self.db
            .query_row("SELECT props FROM notes WHERE path = ?1", [path], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|p| serde_json::from_str(&p).ok())
            .unwrap_or_default()
    }

    /// The next free id of a kind: n1, n2, ...
    pub fn next_id(&self, kind: Kind) -> Result<String> {
        let prefix = kind.id_prefix();
        let mut st =
            self.db.prepare("SELECT id FROM notes WHERE id LIKE ?1 UNION SELECT ref FROM chunks WHERE ref LIKE ?1")?;
        let max = st
            .query_map([format!("{prefix}%")], |r| r.get::<_, String>(0))?
            .flatten()
            .filter_map(|id| id[1..].parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        Ok(format!("{prefix}{}", max + 1))
    }

    pub fn count(&self, sql: &str) -> i64 {
        self.db.query_row(sql, [], |r| r.get(0)).unwrap_or(0)
    }

    /// Indexes one note file now, for a write that must be visible to the
    /// next command at once.
    pub fn refresh(&mut self, project: &Project, rel: &str) -> Result<()> {
        let tx = self.db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        sync::reindex_file(&tx, project, rel)?;
        tx.commit()?;
        Ok(())
    }

    pub fn rebuild(&mut self, project: &Project) -> Result<()> {
        self.db.execute("DELETE FROM meta WHERE key = 'schema'", params![])?;
        self.ensure_schema()?;
        self.sync(project)
    }
}

fn row_to_note(r: &rusqlite::Row) -> rusqlite::Result<NoteRow> {
    Ok(NoteRow {
        path: r.get(0)?,
        id: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
        kind: Kind::parse(&r.get::<_, String>(2)?),
    })
}
