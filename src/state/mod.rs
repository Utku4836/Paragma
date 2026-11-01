//! What cannot be rebuilt from the note files (`.pg/state.db`): the
//! conversations that worked on the project and what they did.

pub mod conversation;
pub mod links;

use crate::output::now;
use crate::project::Project;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::time::Duration;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sessions(
    conversation TEXT PRIMARY KEY,
    folder TEXT,
    started INTEGER,
    last INTEGER
);
CREATE TABLE IF NOT EXISTS events(
    at INTEGER,
    conversation TEXT,
    action TEXT,
    target TEXT,
    detail TEXT
);
CREATE INDEX IF NOT EXISTS events_conversation ON events(conversation, at);
CREATE TABLE IF NOT EXISTS shown(
    conversation TEXT,
    ref TEXT,
    at INTEGER,
    PRIMARY KEY(conversation, ref)
);
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT);
";

pub struct State {
    db: Connection,
}

/// A conversation that worked on the project.
#[derive(Clone, Debug)]
pub struct Session {
    pub folder: String,
    pub last: i64,
    /// Whether its last word was `pg finish`.
    pub finished: bool,
}

/// Something a conversation did.
#[derive(Clone, Debug)]
pub struct Event {
    pub action: String,
    pub target: String,
    pub detail: String,
}

impl State {
    pub fn open(project: &Project) -> Result<State> {
        std::fs::create_dir_all(project.meta_dir())?;
        let db = Connection::open(project.state_path())?;
        db.busy_timeout(Duration::from_secs(60))?;
        db.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        db.execute_batch(SCHEMA)?;
        Ok(State { db })
    }

    /// Notes that this conversation is working on the project.
    pub fn touch_session(&self, folder: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO sessions(conversation, folder, started, last) VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(conversation) DO UPDATE SET last = ?3, folder = ?2",
            params![conversation::key(), folder, now()],
        )?;
        Ok(())
    }

    /// Records what this conversation did (a write, a step, a finish, ...).
    pub fn event(&self, action: &str, target: &str, detail: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO events(at, conversation, action, target, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now(), conversation::key(), action, target, detail],
        )?;
        Ok(())
    }

    /// What this conversation did since its last `pg finish`, oldest first.
    pub fn my_events(&self) -> Result<Vec<Event>> {
        let me = conversation::key();
        let since: i64 = self
            .db
            .query_row(
                "SELECT COALESCE(MAX(rowid), 0) FROM events WHERE conversation = ?1 AND action = 'finish'",
                [me],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let mut st = self.db.prepare(
            "SELECT action, target, detail FROM events WHERE conversation = ?1 AND rowid > ?2 ORDER BY rowid",
        )?;
        let rows = st
            .query_map(params![me, since], |r| Ok(Event { action: r.get(0)?, target: r.get(1)?, detail: r.get(2)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The other conversations that worked here since `since`, latest first.
    pub fn other_sessions(&self, since: i64) -> Result<Vec<Session>> {
        let mut st = self.db.prepare(
            "SELECT s.folder, s.last,
                    (SELECT action FROM events e WHERE e.conversation = s.conversation ORDER BY rowid DESC LIMIT 1)
             FROM sessions s WHERE s.conversation != ?1 AND s.last >= ?2 ORDER BY s.last DESC",
        )?;
        let rows = st
            .query_map(params![conversation::key(), since], |r| {
                Ok(Session {
                    folder: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    last: r.get(1)?,
                    finished: r.get::<_, Option<String>>(2)?.as_deref() == Some("finish"),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Remembers that results were shown to this conversation.
    pub fn shown(&self, refs: &[String]) -> Result<()> {
        let at = now();
        let mut st = self.db.prepare_cached(
            "INSERT INTO shown(conversation, ref, at) VALUES (?1, ?2, ?3)
             ON CONFLICT(conversation, ref) DO UPDATE SET at = ?3",
        )?;
        for r in refs {
            st.execute(params![conversation::key(), r, at])?;
        }
        Ok(())
    }

    /// When a note or rule was last shown to any conversation.
    pub fn last_shown(&self, r: &str) -> Result<Option<i64>> {
        Ok(self.db.query_row("SELECT MAX(at) FROM shown WHERE ref = ?1", [r], |r| r.get(0)).optional()?.flatten())
    }

    /// When the first conversation worked on the project.
    pub fn first_session(&self) -> Result<Option<i64>> {
        Ok(self.db.query_row("SELECT MIN(started) FROM sessions", [], |r| r.get(0)).optional()?.flatten())
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.db.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).ok()
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }
}
