//! Whether what memory showed helped (`outcomes` in state.db). One kind
//! for now, the clearest: a known fix shown for failing tests, and whether
//! those tests passed later in the same conversation (helped) or still
//! failed at its handoff (did not). An agent is told how a fix has done
//! before, and an error record moves a little in search: never more than
//! a fifth, and not before two outcomes.

use super::{State, conversation};
use crate::output::now;
use anyhow::Result;
use rusqlite::params;

/// An outcome counts half after this long.
const HALF_LIFE: f64 = 90.0 * 86_400.0;
/// Outcomes (weighted by age) a record needs before it moves in search.
const ENOUGH: f64 = 2.0;
/// The most a record moves in search: up or down by this share.
const MOST: f64 = 0.2;

/// How a record's fix has done.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    /// Outcomes where it helped, and all, weighted by age.
    pub helped: f64,
    pub all: f64,
    /// Unweighted: times it helped, times shown with an outcome.
    pub helped_count: usize,
    pub count: usize,
    /// The latest outcomes in a row that did not help.
    pub failed_last: usize,
}

impl Track {
    /// How much the record moves in search: 1 without enough outcomes,
    /// otherwise between 1 - MOST and 1 + MOST, by how often it helped (a
    /// Beta(1, 1) prior: one help and one miss before any outcome).
    pub fn factor(&self) -> f64 {
        if self.all < ENOUGH {
            return 1.0;
        }
        let p = (self.helped + 1.0) / (self.all + 2.0);
        1.0 + MOST * (3.0 * (p - 0.5)).tanh() / 1.5f64.tanh()
    }

    /// For an agent, next to the fix: "helped 3 of 4 times", or a warning
    /// when it did not help the last times.
    pub fn describe(&self) -> Option<String> {
        if self.count == 0 {
            return None;
        }
        if self.failed_last >= 2 {
            return Some(format!(
                "did not help the last {} times: the cause may be another this time",
                self.failed_last
            ));
        }
        let times = if self.count == 1 { "time" } else { "times" };
        Some(format!("helped {} of {} {times}", self.helped_count, self.count))
    }
}

impl State {
    /// A known fix was shown for `command` failing `tests`.
    pub fn fix_shown(&self, id: &str, command: &str, tests: &[String]) -> Result<()> {
        self.event("fix-shown", id, &format!("{}\t{}", crate::testrun::normalized(command), tests.join("\t")))
    }

    /// `command` passes now for the tests in `fixed`: the fixes shown to
    /// this conversation for them helped.
    pub fn tests_fixed(&self, command: &str, fixed: &[String]) -> Result<()> {
        let command = crate::testrun::normalized(command);
        for (id, cmd, tests) in self.pending()? {
            if cmd == command && tests.iter().any(|t| fixed.contains(t)) {
                self.outcome(&id, &cmd, true)?;
            }
        }
        Ok(())
    }

    /// At a handoff: the fixes shown whose tests still fail did not help.
    /// Call before the handoff's own event.
    pub fn outcomes_at_handoff(&self) -> Result<()> {
        for (id, cmd, tests) in self.pending()? {
            let last = self.my_runs(&cmd)?;
            if last.is_some_and(|failing| tests.iter().any(|t| failing.contains(t))) {
                self.outcome(&id, &cmd, false)?;
            }
        }
        Ok(())
    }

    /// Whether any outcome was kept in this project.
    pub fn has_outcomes(&self) -> bool {
        self.db.query_row("SELECT EXISTS(SELECT 1 FROM outcomes)", [], |r| r.get(0)).unwrap_or(false)
    }

    /// How the fix of a record has done.
    pub fn track(&self, id: &str) -> Result<Track> {
        let mut st =
            self.db.prepare_cached("SELECT at, helped FROM outcomes WHERE ref = ?1 ORDER BY at DESC, rowid DESC")?;
        let rows = st.query_map([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? == 1)))?;
        let now = now() as f64;
        let mut t = Track::default();
        let mut streak = true;
        for r in rows {
            let (at, helped) = r?;
            let w = 0.5f64.powf(((now - at as f64).max(0.0)) / HALF_LIFE);
            t.all += w;
            t.count += 1;
            if helped {
                t.helped += w;
                t.helped_count += 1;
                streak = false;
            } else if streak {
                t.failed_last += 1;
            }
        }
        Ok(t)
    }

    /// The fixes shown to this conversation since its last handoff, still
    /// without an outcome: (record, command, tests).
    fn pending(&self) -> Result<Vec<(String, String, Vec<String>)>> {
        let since = self.my_last("finish").unwrap_or(0);
        let mut st = self.db.prepare_cached(
            "SELECT e.target, e.detail FROM events e WHERE e.conversation = ?1 AND e.action = 'fix-shown' AND e.at >= ?2
             AND NOT EXISTS (SELECT 1 FROM outcomes o WHERE o.conversation = e.conversation AND o.ref = e.target AND o.at >= ?2)
             ORDER BY e.rowid",
        )?;
        let rows = st
            .query_map(params![conversation::key(), since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out: Vec<(String, String, Vec<String>)> = Vec::new();
        for (id, detail) in rows {
            let mut parts = detail.split('\t');
            let cmd = parts.next().unwrap_or("").to_string();
            let tests: Vec<String> = parts.filter(|t| !t.is_empty()).map(str::to_string).collect();
            if !out.iter().any(|(i, c, _)| *i == id && *c == cmd) {
                out.push((id, cmd, tests));
            }
        }
        Ok(out)
    }

    /// The tests failing in this conversation's last run of a command (by
    /// its normalized form); None when it did not run it.
    fn my_runs(&self, command: &str) -> Result<Option<Vec<String>>> {
        let mut st = self
            .db
            .prepare_cached("SELECT command, failing FROM runs WHERE conversation = ?1 ORDER BY rowid DESC LIMIT 50")?;
        let rows = st
            .query_map([conversation::key()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .find(|(c, _)| crate::testrun::normalized(c) == command)
            .map(|(_, failing)| failing.lines().filter(|l| !l.is_empty()).map(str::to_string).collect()))
    }

    fn outcome(&self, id: &str, command: &str, helped: bool) -> Result<()> {
        self.db.execute(
            "INSERT INTO outcomes(ref, at, conversation, command, helped) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, now(), conversation::key(), command, i64::from(helped)],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(helped: usize, count: usize, failed_last: usize) -> Track {
        Track { helped: helped as f64, all: count as f64, helped_count: helped, count, failed_last }
    }

    #[test]
    fn little_data_moves_nothing_and_much_moves_a_little() {
        assert_eq!(track(0, 0, 0).factor(), 1.0);
        assert_eq!(track(1, 1, 0).factor(), 1.0);
        let good = track(5, 5, 0).factor();
        let bad = track(0, 5, 5).factor();
        assert!(good > 1.1 && good <= 1.0 + MOST, "{good}");
        assert!((1.0 - MOST..0.9).contains(&bad), "{bad}");
        assert!((track(3, 6, 0).factor() - 1.0).abs() < 1e-9);
        // never past the bound, however many
        assert!(track(1000, 1000, 0).factor() <= 1.0 + MOST + 1e-9);
    }

    #[test]
    fn what_an_agent_is_told() {
        assert_eq!(track(0, 0, 0).describe(), None);
        assert_eq!(track(1, 1, 0).describe().unwrap(), "helped 1 of 1 time");
        assert_eq!(track(3, 4, 1).describe().unwrap(), "helped 3 of 4 times");
        assert!(track(1, 3, 2).describe().unwrap().starts_with("did not help the last 2 times"));
    }
}
