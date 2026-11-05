//! What cannot be rebuilt from the note files (`.pg/state.db`): the
//! conversations that worked on the project and what they did.

pub mod conversation;
pub mod links;
pub mod migrate;
pub mod outcome;

use crate::output::now;
use crate::project::Project;
use crate::testrun::Run;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

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
    pub detail: String,
}

impl State {
    pub fn open(project: &Project) -> Result<State> {
        let path = project.state_path();
        let mut db = crate::db::open(&path)?;
        migrate::run(&mut db, &path)?;
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
    /// `skip`: finishes passed over (1: since the one before the last, for a
    /// handoff that replaces the last one).
    pub fn my_events(&self, skip: usize) -> Result<Vec<Event>> {
        let me = conversation::key();
        let since: i64 = self
            .db
            .query_row(
                "SELECT rowid FROM events WHERE conversation = ?1 AND action = 'finish' ORDER BY rowid DESC LIMIT 1 OFFSET ?2",
                params![me, skip as i64],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let mut st = self
            .db
            .prepare("SELECT action, detail FROM events WHERE conversation = ?1 AND rowid > ?2 ORDER BY rowid")?;
        let rows = st
            .query_map(params![me, since], |r| Ok(Event { action: r.get(0)?, detail: r.get(1)? }))?
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

    /// Remembers that results were shown to this conversation, in full or
    /// as one line.
    pub fn shown(&self, refs: &[String], full: bool) -> Result<()> {
        let at = now();
        let full_at = if full { at } else { 0 };
        let mut st = self.db.prepare_cached(
            "INSERT INTO shown(conversation, ref, at, full_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(conversation, ref) DO UPDATE SET at = ?3, full_at = MAX(full_at, ?4)",
        )?;
        for r in refs {
            st.execute(params![conversation::key(), r, at, full_at])?;
        }
        Ok(())
    }

    /// What this conversation was shown in full since `since`.
    pub fn shown_in_full(&self, since: i64) -> Result<std::collections::HashSet<String>> {
        let mut st = self.db.prepare_cached("SELECT ref FROM shown WHERE conversation = ?1 AND full_at >= ?2")?;
        let rows = st.query_map(params![conversation::key(), since], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Whether this conversation was shown a note or rule since `since`.
    pub fn shown_to_me(&self, r: &str, since: i64) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM shown WHERE conversation = ?1 AND ref = ?2 AND at >= ?3",
                params![conversation::key(), r, since],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// When a note or rule was last shown to any conversation.
    pub fn last_shown(&self, r: &str) -> Result<Option<i64>> {
        Ok(self.db.query_row("SELECT MAX(at) FROM shown WHERE ref = ?1", [r], |r| r.get(0)).optional()?.flatten())
    }

    /// When the first conversation worked on the project.
    pub fn first_session(&self) -> Result<Option<i64>> {
        Ok(self.db.query_row("SELECT MIN(started) FROM sessions", [], |r| r.get(0)).optional()?.flatten())
    }

    pub fn record_run(&self, r: &Run) -> Result<()> {
        self.db.execute(
            "INSERT INTO runs(at, conversation, command, exit, seconds, failing, passed, errors, changed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                r.at,
                conversation::key(),
                r.command,
                r.exit,
                r.seconds,
                r.failing.join("\n"),
                r.passed as i64,
                r.errors.join("\n"),
                r.changed.join("\n")
            ],
        )?;
        Ok(())
    }

    /// Earlier runs, newest first: of one command, or of any.
    pub fn runs(&self, command: Option<&str>, limit: usize) -> Result<Vec<Run>> {
        let sql = "SELECT at, command, exit, seconds, failing, passed, errors, changed FROM runs
                   WHERE ?1 IS NULL OR command = ?1 ORDER BY rowid DESC LIMIT ?2";
        let mut st = self.db.prepare_cached(sql)?;
        let lines = |s: String| -> Vec<String> { s.lines().filter(|l| !l.is_empty()).map(str::to_string).collect() };
        let rows = st
            .query_map(params![command, limit as i64], |r| {
                Ok(Run {
                    at: r.get(0)?,
                    command: r.get(1)?,
                    exit: r.get(2)?,
                    seconds: r.get(3)?,
                    failing: lines(r.get(4)?),
                    passed: r.get::<_, i64>(5)? as usize,
                    errors: lines(r.get(6)?),
                    changed: lines(r.get(7)?),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// This conversation's last run since its last `pg finish`.
    pub fn my_last_run(&self) -> Result<Option<Run>> {
        let me = conversation::key();
        let since: i64 = self
            .db
            .query_row(
                "SELECT COALESCE(MAX(at), 0) FROM events WHERE conversation = ?1 AND action = 'finish'",
                [me],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let command: Option<String> = self
            .db
            .query_row(
                "SELECT command FROM runs WHERE conversation = ?1 AND at >= ?2 ORDER BY rowid DESC LIMIT 1",
                params![me, since],
                |r| r.get(0),
            )
            .optional()?;
        match command {
            Some(c) => Ok(self.runs(Some(&c), 1)?.pop()),
            None => Ok(None),
        }
    }

    /// What this conversation did `action` to since `since`, each once, in
    /// order (the files it edited, ...).
    pub fn my_targets_since(&self, action: &str, since: i64) -> Result<Vec<String>> {
        let mut st = self.db.prepare_cached(
            "SELECT target FROM events WHERE conversation = ?1 AND action = ?2 AND at >= ?3 GROUP BY target ORDER BY MIN(rowid)",
        )?;
        let rows = st
            .query_map(params![conversation::key(), action, since], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(rows)
    }

    /// When this conversation last did `action` (an "open": its start, or
    /// its brief again after it was compacted).
    pub fn my_last(&self, action: &str) -> Option<i64> {
        self.db
            .query_row(
                "SELECT MAX(at) FROM events WHERE conversation = ?1 AND action = ?2",
                params![conversation::key(), action],
                |r| r.get(0),
            )
            .ok()
            .flatten()
    }

    /// The notes kept for after this conversation's edit (hook.rs), taken:
    /// (the files, the note ids); None when nothing waits.
    pub fn take_after_edit(&self) -> Result<Option<(String, Vec<String>)>> {
        let row: Option<(i64, String, String)> = self
            .db
            .query_row(
                "SELECT rowid, target, detail FROM events WHERE conversation = ?1 AND action = 'after-edit' ORDER BY rowid DESC LIMIT 1",
                [conversation::key()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let Some((rowid, files, ids)) = row else { return Ok(None) };
        self.db.execute("UPDATE events SET action = 'after-edit-shown' WHERE rowid = ?1", [rowid])?;
        Ok(Some((files, ids.split_whitespace().map(str::to_string).collect())))
    }

    /// When the last `pg finish` of any conversation was.
    pub fn last_finish(&self) -> Option<i64> {
        self.db.query_row("SELECT MAX(at) FROM events WHERE action = 'finish'", [], |r| r.get(0)).ok().flatten()
    }

    /// The ids written to memory since a time, by any conversation.
    pub fn written_since(&self, time: i64) -> Result<Vec<String>> {
        let mut st =
            self.db.prepare_cached("SELECT detail FROM events WHERE action = 'write' AND at > ?1 ORDER BY rowid")?;
        let mut ids: Vec<String> = Vec::new();
        for d in st.query_map([time], |r| r.get::<_, String>(0))? {
            let d = d?;
            let id = d.split(' ').next().unwrap_or("").to_string();
            if !id.is_empty() && !ids.contains(&id) {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    /// The commands run here, most run first: (command, runs, last exit).
    pub fn commands(&self, limit: usize) -> Result<Vec<(String, i64, i32)>> {
        let mut st = self.db.prepare_cached(
            "SELECT command, COUNT(*), (SELECT exit FROM runs r2 WHERE r2.command = r.command ORDER BY rowid DESC LIMIT 1)
             FROM runs r GROUP BY command ORDER BY COUNT(*) DESC, MAX(at) DESC LIMIT ?1",
        )?;
        let rows = st
            .query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
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
