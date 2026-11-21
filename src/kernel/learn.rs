//! The experts that write (decision level 1): what the facts make worth
//! keeping in memory, decided here, written by the shell (cli/act.rs), each
//! in the event log and taken back by `pg undo`. They write only facts
//! pg saw, marked as written by pg, and never change a line someone wrote.
//! - rule candidate: a known error shown in several conversations keeps
//!   coming back: a draft rule (not binding until confirmed);
//! - flaky test: a test that failed and passed with the same code: a draft
//!   record, so every computer knows it (runs stay on this one);
//! - ineffective fix: a fix shown whose tests still failed at the handoff:
//!   a dated line on its record;
//! - run recipe: the commands that passed here, in the project note, so a
//!   new computer knows them too.

use super::Facts;
use super::experts::{Purpose, purpose};
use crate::state::journal::Did;
use crate::testrun::{Run, normalized};
use std::collections::{BTreeMap, BTreeSet};

/// Conversations a known error is shown in before it is a rule candidate.
pub const RULE_AFTER: usize = 3;
/// Runs of a command looked at for a flaky test.
pub const FLAKY_RUNS: usize = 10;
/// Failures of a test among those before it is called flaky.
pub const FLAKY_FAILS: usize = 2;

/// A known error that keeps coming back.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub error: String,
    /// The conversations it was shown in.
    pub conversations: usize,
}

/// Errors whose known fix was shown in `RULE_AFTER` conversations or more,
/// and that no rule names yet (`cited`: the error ids rules name).
pub fn rule_candidates(f: &Facts, cited: &[String]) -> Vec<Candidate> {
    let mut by: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for e in &f.journal {
        if let Did::FixShown { id, .. } = &e.did {
            by.entry(id.as_str()).or_default().insert(e.conversation.as_str());
        }
    }
    by.into_iter()
        .filter(|(id, convs)| convs.len() >= RULE_AFTER && !cited.iter().any(|c| c == id))
        .map(|(id, convs)| Candidate { error: id.to_string(), conversations: convs.len() })
        .collect()
}

/// The draft rule's line for a candidate, from its record's title and fix.
pub fn rule_text(c: &Candidate, title: &str, fix: &str) -> String {
    let fix = fix.split_whitespace().collect::<Vec<_>>().join(" ");
    let fix = fix.trim_end_matches('.');
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if fix.is_empty() {
        format!("(draft) Watch for: {title} (from {}, in {} conversations)", c.error, c.conversations)
    } else {
        format!("(draft) {fix}: {title} keeps coming back (from {}, in {} conversations)", c.error, c.conversations)
    }
}

/// A test that failed and passed with the same code.
#[derive(Clone, Debug, PartialEq)]
pub struct Flaky {
    pub test: String,
    pub command: String,
    pub fails: usize,
    pub runs: usize,
}

/// Tests that failed and passed with the same code (the same commit and
/// the same changes, `Run::code`), among the last `FLAKY_RUNS` runs of a
/// command, failing `FLAKY_FAILS` times or more: not a change of the code
/// made them fail. `runs`: newest first.
pub fn flaky(runs: &[Run]) -> Vec<Flaky> {
    let mut by: BTreeMap<String, Vec<&Run>> = BTreeMap::new();
    for r in runs {
        let list = by.entry(normalized(&r.command)).or_default();
        if list.len() < FLAKY_RUNS {
            list.push(r);
        }
    }
    let mut out = Vec::new();
    for (command, list) in by {
        let mut tests: Vec<&str> = list.iter().flat_map(|r| r.failing.iter().map(String::as_str)).collect();
        tests.sort_unstable();
        tests.dedup();
        for t in tests {
            let fails = list.iter().filter(|r| r.failing.iter().any(|x| x == t)).count();
            if fails < FLAKY_FAILS {
                continue;
            }
            // a failing run and a passing one (the test ran and did not fail)
            // with the same code
            let same_code = list.iter().any(|a| {
                !a.code.is_empty()
                    && a.failing.iter().any(|x| x == t)
                    && list
                        .iter()
                        .any(|b| b.code == a.code && b.exit == 0 && ran_tests(b) && !b.failing.iter().any(|x| x == t))
            });
            if same_code {
                out.push(Flaky { test: t.to_string(), command: command.clone(), fails, runs: list.len() });
            }
        }
    }
    out
}

fn ran_tests(r: &Run) -> bool {
    r.passed > 0 || !r.failing.is_empty()
}

/// The line for a flaky test's draft record (its "error").
pub fn flaky_line(f: &Flaky) -> String {
    format!("flaky test: {} ({})", f.test, f.command)
}

/// The dated line a fix that did not help gets on its record.
pub fn outcome_line(date: &str, command: &str, tests: &[String]) -> String {
    let shown: Vec<&str> = tests.iter().take(3).map(String::as_str).collect();
    let more = tests.len().saturating_sub(3);
    format!(
        "- {date}: did not help: {}{} still failing at the handoff ({}) (written by pg)",
        shown.join(", "),
        if more > 0 { format!(" +{more}") } else { String::new() },
        command
    )
}

/// The heading of the project note's section pg keeps the recipes in.
pub const RECIPE_SECTION: &str = "How it runs";

/// The recipes section: for tests, a build and checks, the command used
/// most that passed (as `recipes` says them), one line each. None when
/// nothing passed yet.
pub fn recipe_section(f: &Facts) -> Option<String> {
    let mut seen: Vec<(&str, usize, i64, bool)> = Vec::new(); // command, passes, newest, ran tests
    for r in &f.runs {
        match seen.iter_mut().find(|s| s.0 == r.command) {
            Some(s) => {
                s.1 += usize::from(r.exit == 0);
                s.3 |= ran_tests(r);
            }
            None => seen.push((&r.command, usize::from(r.exit == 0), r.at, ran_tests(r))),
        }
    }
    let mut best: Vec<(Purpose, &str, usize, i64)> = Vec::new();
    for (command, passes, newest, tests) in seen.into_iter().filter(|s| s.1 > 0) {
        let p = if tests { Purpose::Tests } else { purpose(command) };
        if p == Purpose::Other {
            continue;
        }
        match best.iter_mut().find(|b| b.0 == p) {
            Some(b) if (passes, newest) > (b.2, b.3) => *b = (p, command, passes, newest),
            Some(_) => {}
            None => best.push((p, command, passes, newest)),
        }
    }
    if best.is_empty() {
        return None;
    }
    best.sort_by_key(|b| b.0);
    let lines: Vec<String> = best
        .iter()
        .map(|(p, c, _, _)| {
            let what = match p {
                Purpose::Tests => "Tests",
                Purpose::Build => "Build",
                Purpose::Check => "Checks",
                Purpose::Other => "Other",
            };
            format!("- {what}: `{c}`")
        })
        .collect();
    Some(format!("{}\n(kept by pg from the commands that passed here)", lines.join("\n")))
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/kernel/learn.rs"]
mod tests;
