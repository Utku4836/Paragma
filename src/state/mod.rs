//! What cannot be rebuilt from the note files (`.pg/state.db`): the
//! conversations that worked on the project and what they did.

pub mod conversation;
pub mod links;

use crate::project::Project;
use anyhow::Result;
use rusqlite::{Connection, params};
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
";

pub struct State {
    db: Connection,
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
        let now = now();
        self.db.execute(
            "INSERT INTO sessions(conversation, folder, started, last) VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(conversation) DO UPDATE SET last = ?3, folder = ?2",
            params![conversation::key(), folder, now],
        )?;
        Ok(())
    }

    /// Records what this conversation did (a write, a test run, ...).
    pub fn event(&self, action: &str, target: &str, detail: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO events(at, conversation, action, target, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now(), conversation::key(), action, target, detail],
        )?;
        Ok(())
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
