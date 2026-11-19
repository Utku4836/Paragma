//! The event log: everything a conversation did that pg saw, in order, kept
//! in `state.db`'s `events` table. Written only here and only appended to
//! (one row per event; a later event can say what an earlier one became,
//! never by changing it), so many pg at once never overwrite each other and
//! what happened can always be read back: what a conversation did since its
//! last handoff, what a conversation cut short left, which files it edited.
//!
//! The rows keep the plain form of earlier versions (action, target,
//! detail), which older and newer pg read alike.

use super::{State, conversation};
use crate::output::now;
use anyhow::Result;
use rusqlite::params;

/// What a conversation did.
#[derive(Clone, Debug, PartialEq)]
pub enum Did {
    /// Opened the project (or got its brief again after it was compacted).
    Open,
    /// Searched: the query and the ids shown.
    Find {
        query: String,
        ids: Vec<String>,
    },
    /// Read a record right after a search: the search was about it.
    Picked {
        query: String,
        id: String,
    },
    /// Wrote a record or a note: what kind (rule, decision, a note's path),
    /// its id and what happened ("created", "updated: ...").
    Write {
        kind: String,
        id: String,
        what: String,
    },
    /// Saved a secret (its title only).
    Secret {
        title: String,
        new: bool,
    },
    /// Added a step to the plan.
    Step {
        text: String,
    },
    /// Handed off.
    Finish {
        summary: String,
    },
    Mv {
        from: String,
        to: String,
    },
    Rm {
        what: String,
    },
    /// Changed a code file (seen by a hook).
    Edit {
        file: String,
    },
    /// Notes kept to show after an edit (a hook), and when they were shown.
    AfterEdit {
        files: String,
        ids: Vec<String>,
    },
    AfterEditShown {
        of: i64,
    },
    /// Ran a command: passed, or how many tests failed.
    Run {
        command: String,
        passed: bool,
        failing: usize,
    },
    /// A run fixed the tests a failing run had: its first error line.
    Fixed {
        line: String,
        tests: Vec<String>,
    },
    /// A known fix was shown for a failing command.
    FixShown {
        id: String,
        command: String,
        tests: Vec<String>,
    },
    /// Something offered to the agent once (a plan, a rule).
    Offered {
        what: String,
        about: String,
    },
    /// Asked once to hand off (the Stop hook).
    AskedFinish,
    /// Took a step of the plan: the others see it taken.
    Take {
        step: usize,
    },
    /// A short note for the other conversations here.
    Told {
        text: String,
    },
    /// Something pg did by itself, inside memory (decision level 1): what
    /// ("handoff", "draft", "closed"), what it is about, and what undoing it
    /// needs. `pg undo` lists and takes back these.
    Auto {
        what: String,
        target: String,
        detail: String,
    },
    /// An automatic action taken back (`pg undo`): its event's number.
    Undone {
        of: i64,
    },
    /// An action of another version of pg, kept as it was.
    Other {
        action: String,
        target: String,
        detail: String,
    },
}

/// One event, with when and by which conversation.
#[derive(Clone, Debug)]
pub struct Entry {
    pub seq: i64,
    pub at: i64,
    pub conversation: String,
    pub did: Did,
}

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

impl Did {
    /// The row: action, target, detail.
    fn row(&self) -> (String, String, String) {
        let s = |x: &str| x.to_string();
        match self {
            Did::Open => (s("open"), String::new(), String::new()),
            Did::Find { query, ids } => (s("find"), query.clone(), ids.join(" ")),
            Did::Picked { query, id } => (s("picked"), query.clone(), id.clone()),
            Did::Write { kind, id, what } => (s("write"), kind.clone(), format!("{id} {what}")),
            Did::Secret { title, new } => (s("secret"), title.clone(), s(if *new { "added" } else { "replaced" })),
            Did::Step { text } => (s("step"), text.clone(), String::new()),
            Did::Finish { summary } => (s("finish"), String::new(), summary.clone()),
            Did::Mv { from, to } => (s("mv"), from.clone(), to.clone()),
            Did::Rm { what } => (s("rm"), what.clone(), String::new()),
            Did::Edit { file } => (s("edit"), file.clone(), String::new()),
            Did::AfterEdit { files, ids } => (s("after-edit"), files.clone(), ids.join(" ")),
            Did::AfterEditShown { of } => (s("after-edit-shown"), of.to_string(), String::new()),
            Did::Run { command, passed, failing } => {
                (s("run"), command.clone(), if *passed { s("pass") } else { format!("fail {failing}") })
            }
            Did::Fixed { line, tests } => (s("fixed"), line.clone(), tests.join(", ")),
            Did::FixShown { id, command, tests } => {
                (s("fix-shown"), id.clone(), format!("{}\t{}", crate::testrun::normalized(command), tests.join("\t")))
            }
            Did::Offered { what, about } => (format!("{what}-offered"), about.clone(), String::new()),
            Did::AskedFinish => (s("ask-finish"), String::new(), String::new()),
            Did::Take { step } => (s("take"), step.to_string(), String::new()),
            Did::Told { text } => (s("tell"), text.clone(), String::new()),
            Did::Auto { what, target, detail } => (s("auto"), what.clone(), format!("{target}\t{detail}")),
            Did::Undone { of } => (s("undone"), of.to_string(), String::new()),
            Did::Other { action, target, detail } => (action.clone(), target.clone(), detail.clone()),
        }
    }

    /// The event a row holds (rows of every version of pg).
    pub fn from_row(action: &str, target: &str, detail: &str) -> Did {
        let (t, d) = (target.to_string(), detail.to_string());
        match action {
            "open" => Did::Open,
            "find" => Did::Find { query: t, ids: words(detail) },
            "picked" => Did::Picked { query: t, id: d },
            "write" => {
                let (id, what) = detail.split_once(' ').unwrap_or((detail, ""));
                Did::Write { kind: t, id: id.to_string(), what: what.to_string() }
            }
            "secret" => Did::Secret { title: t, new: detail == "added" },
            "step" => Did::Step { text: t },
            "finish" => Did::Finish { summary: d },
            "mv" => Did::Mv { from: t, to: d },
            "rm" => Did::Rm { what: t },
            "edit" => Did::Edit { file: t },
            "after-edit" => Did::AfterEdit { files: t, ids: words(detail) },
            "after-edit-shown" => Did::AfterEditShown { of: target.parse().unwrap_or(0) },
            "run" => Did::Run {
                command: t,
                passed: detail == "pass",
                failing: detail.strip_prefix("fail").map(|n| n.trim().parse().unwrap_or(0)).unwrap_or(0),
            },
            "fixed" => Did::Fixed {
                line: t,
                tests: detail.split(", ").filter(|x| !x.is_empty()).map(str::to_string).collect(),
            },
            "fix-shown" => {
                let mut parts = detail.split('\t');
                let command = parts.next().unwrap_or("").to_string();
                Did::FixShown { id: t, command, tests: parts.filter(|x| !x.is_empty()).map(str::to_string).collect() }
            }
            "ask-finish" => Did::AskedFinish,
            "take" => Did::Take { step: target.parse().unwrap_or(0) },
            "tell" => Did::Told { text: t },
            "auto" => {
                let (tg, de) = detail.split_once('\t').unwrap_or((detail, ""));
                Did::Auto { what: t, target: tg.to_string(), detail: de.to_string() }
            }
            "undone" => Did::Undone { of: target.parse().unwrap_or(0) },
            a if a.ends_with("-offered") => Did::Offered { what: a.trim_end_matches("-offered").to_string(), about: t },
            _ => Did::Other { action: action.to_string(), target: t, detail: d },
        }
    }

    /// Whether it changed the project (memory or code), as opposed to
    /// looking at it.
    pub fn changes(&self) -> bool {
        matches!(
            self,
            Did::Write { .. }
                | Did::Secret { .. }
                | Did::Step { .. }
                | Did::Mv { .. }
                | Did::Rm { .. }
                | Did::Edit { .. }
                | Did::Run { .. }
        )
    }
}

impl State {
    /// Appends what this conversation did to the log.
    pub fn record(&self, did: Did) -> Result<()> {
        let (action, target, detail) = did.row();
        self.db.execute(
            "INSERT INTO events(at, conversation, action, target, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now(), conversation::key(), action, target, detail],
        )?;
        Ok(())
    }

    /// Appends what this conversation did, and gives its number.
    pub fn record_numbered(&self, did: Did) -> Result<i64> {
        self.record(did)?;
        Ok(self.db.last_insert_rowid())
    }

    /// The log of one kind of event since a time, oldest first.
    pub fn journal_of(&self, action: &str, since: i64) -> Result<Vec<Entry>> {
        Ok(self.journal(since)?.into_iter().filter(|e| e.did.row().0 == action).collect())
    }

    /// The log since a time, oldest first, of every conversation.
    pub fn journal(&self, since: i64) -> Result<Vec<Entry>> {
        let mut st = self.db.prepare_cached(
            "SELECT rowid, at, conversation, action, target, detail FROM events WHERE at >= ?1 ORDER BY rowid",
        )?;
        let rows = st.query_map([since], |r| {
            Ok(Entry {
                seq: r.get(0)?,
                at: r.get(1)?,
                conversation: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                did: Did::from_row(
                    &r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    &r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    &r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                ),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/state/journal.rs"]
mod tests;
