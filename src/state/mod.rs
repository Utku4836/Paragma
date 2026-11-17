//! What cannot be rebuilt from the note files (`.pg/state.db`): the
//! conversations that worked on the project and what they did.

pub mod conversation;
pub mod journal;
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

/// Something a conversation did.
#[derive(Clone, Debug)]
pub struct Event {
    pub action: String,
    pub detail: String,
}

impl State {
    pub fn open(project: &Project) -> Result<State> {
        let path = project.state_path();
        // what it holds helps, but pg works without it: a damaged file is
        // set aside and a new one started
        crate::db::open_or_renew(&path, |mut db| {
            migrate::run(&mut db, &path)?;
            Ok(State { db })
        })
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

    /// What this conversation's last `pg find` asked and showed, when it was
    /// in the last `within` seconds: (query, ids shown).
    pub fn last_find(&self, within: i64) -> Option<(String, Vec<String>)> {
        self.db
            .query_row(
                "SELECT target, detail FROM events WHERE conversation = ?1 AND action = 'find' AND at >= ?2 ORDER BY rowid DESC LIMIT 1",
                params![conversation::key(), now() - within],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .ok()
            .map(|(q, ids)| (q, ids.split(' ').filter(|s| !s.is_empty()).map(str::to_string).collect()))
    }

    /// The records picked after a search (read right after it), lately:
    /// (query, id).
    pub fn picks(&self, last: usize) -> Vec<(String, String)> {
        let Ok(mut st) = self
            .db
            .prepare_cached("SELECT target, detail FROM events WHERE action = 'picked' ORDER BY rowid DESC LIMIT ?1")
        else {
            return Vec::new();
        };
        st.query_map([last as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// The records written in the conversations that wrote `id` (on this
    /// computer: the history is not synced).
    pub fn written_with(&self, id: &str) -> Result<Vec<String>> {
        let mut st = self.db.prepare_cached(
            "SELECT DISTINCT e.detail FROM events e WHERE e.action = 'write' AND e.conversation IN \
             (SELECT conversation FROM events WHERE action = 'write' AND (detail = ?1 OR detail LIKE ?2)) ORDER BY e.rowid",
        )?;
        let rows = st.query_map(params![id, format!("{id} %")], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for d in rows.flatten() {
            let x = d.split(' ').next().unwrap_or("").to_string();
            if !x.is_empty() && x != id && !out.contains(&x) {
                out.push(x);
            }
        }
        Ok(out)
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

    /// The folder each conversation worked in that was here since `since`.
    pub fn folders(&self, since: i64) -> Result<std::collections::HashMap<String, String>> {
        let mut st = self.db.prepare_cached("SELECT conversation, folder FROM sessions WHERE last >= ?1")?;
        let rows = st.query_map([since], |r| {
            let folder = r.get::<_, Option<String>>(1)?.unwrap_or_default();
            // the folder's name says enough: "shop-api", not the whole path
            let name =
                std::path::Path::new(&folder).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            Ok((r.get::<_, String>(0)?, name))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The keys of the hints (kernel) said to this conversation.
    pub fn said(&self) -> Result<std::collections::HashSet<String>> {
        let mut st = self.db.prepare_cached("SELECT ref FROM shown WHERE conversation = ?1 AND ref LIKE 'hint:%'")?;
        let rows = st.query_map([conversation::key()], |r| r.get::<_, String>(0))?;
        Ok(rows.flatten().map(|r| r.trim_start_matches("hint:").to_string()).collect())
    }

    /// The hints said to every conversation since a time (kernel/adapt.rs).
    pub fn said_log(&self, since: i64) -> Result<Vec<crate::kernel::adapt::Said>> {
        let mut st = self.db.prepare_cached(
            "SELECT conversation, ref, at FROM shown WHERE ref LIKE 'hint:%' AND at >= ?1 ORDER BY at",
        )?;
        let rows = st.query_map([since], |r| {
            Ok(crate::kernel::adapt::Said {
                conversation: r.get(0)?,
                key: r.get::<_, String>(1)?.trim_start_matches("hint:").to_string(),
                at: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every time a note or rule was used that this computer saw: shown,
    /// read in full, picked after a search, written.
    pub fn uses(&self, id: &str) -> Result<Vec<i64>> {
        let mut st = self.db.prepare_cached(
            "SELECT at FROM shown WHERE ref = ?1
             UNION ALL SELECT full_at FROM shown WHERE ref = ?1 AND full_at > 0
             UNION ALL SELECT at FROM events WHERE action = 'picked' AND detail = ?1
             UNION ALL SELECT at FROM events WHERE action = 'write' AND (detail = ?1 OR detail LIKE ?2)",
        )?;
        let rows = st.query_map(params![id, format!("{id} %")], |r| r.get::<_, i64>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Remembers hints as said to this conversation.
    pub fn say(&self, keys: &[String]) -> Result<()> {
        self.shown(&keys.iter().map(|k| format!("hint:{k}")).collect::<Vec<_>>())
    }

    /// Remembers that results were shown to this conversation as one line.
    pub fn shown(&self, refs: &[String]) -> Result<()> {
        let mut st = self.db.prepare_cached(
            "INSERT INTO shown(conversation, ref, at) VALUES (?1, ?2, ?3)
             ON CONFLICT(conversation, ref) DO UPDATE SET at = ?3",
        )?;
        let at = now();
        for r in refs {
            st.execute(params![conversation::key(), r, at])?;
        }
        Ok(())
    }

    /// Remembers that notes or rules were shown to this conversation in
    /// full, each with its version (`Index::versions`).
    pub fn shown_in_full(&self, refs: &[(String, i64)]) -> Result<()> {
        let mut st = self.db.prepare_cached(
            "INSERT INTO shown(conversation, ref, at, full_at, version) VALUES (?1, ?2, ?3, ?3, ?4)
             ON CONFLICT(conversation, ref) DO UPDATE SET at = ?3, full_at = ?3, version = ?4",
        )?;
        let at = now();
        for (r, version) in refs {
            st.execute(params![conversation::key(), r, at, version])?;
        }
        Ok(())
    }

    /// What this conversation was shown in full since `since`, with the
    /// version it saw.
    pub fn seen_in_full(&self, since: i64) -> Result<std::collections::HashMap<String, i64>> {
        let mut st =
            self.db.prepare_cached("SELECT ref, version FROM shown WHERE conversation = ?1 AND full_at >= ?2")?;
        let rows = st
            .query_map(params![conversation::key(), since], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
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

    /// When the first conversation worked on the project.
    pub fn first_session(&self) -> Result<Option<i64>> {
        Ok(self.db.query_row("SELECT MIN(started) FROM sessions", [], |r| r.get(0)).optional()?.flatten())
    }

    pub fn record_run(&self, r: &Run) -> Result<()> {
        self.db.execute(
            "INSERT INTO runs(at, conversation, command, exit, seconds, failing, passed, errors, changed, symbols)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                r.at,
                conversation::key(),
                r.command,
                r.exit,
                r.seconds,
                r.failing.join("\n"),
                r.passed as i64,
                r.errors.join("\n"),
                r.changed.join("\n"),
                r.symbols.join("\n")
            ],
        )?;
        Ok(())
    }

    /// Earlier runs, newest first: of one command, or of any.
    pub fn runs(&self, command: Option<&str>, limit: usize) -> Result<Vec<Run>> {
        let sql = "SELECT at, command, exit, seconds, failing, passed, errors, changed, symbols FROM runs
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
                    symbols: lines(r.get(8)?),
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
        // shown once: said by a later event, the log is only appended to
        let shown: bool = self.db.query_row(
            "SELECT COUNT(*) > 0 FROM events WHERE conversation = ?1 AND action = 'after-edit-shown' AND target = ?2",
            params![conversation::key(), rowid.to_string()],
            |r| r.get(0),
        )?;
        if shown {
            return Ok(None);
        }
        self.record(journal::Did::AfterEditShown { of: rowid })?;
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
