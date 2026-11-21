//! The experts that only say (decision level 0): a hint with how to act on
//! it, never an action. Each is measured on the scenario sets; one that
//! misfires is turned off (kernel/gate.rs).
//! - stuck step: open while several conversations handed off without
//!   touching it;
//! - stale drafts: drafts pg wrote that nobody confirmed in two weeks;
//! - archive revival: an archived note read again in several conversations;
//! - thin handoff: a handoff that says what was done, not what is next;
//! - rule guard: a change that adds what a rule of that code forbids.

use super::{Facts, Hint};
use crate::output::{count, one_line};
use crate::state::journal::Did;
use std::collections::BTreeSet;

/// Handoffs of other conversations that left an open step untouched
/// before it is asked about.
pub const STUCK_AFTER: usize = 3;
/// Conversations that read an archived note before it is worth bringing
/// back.
pub const REVIVE_AFTER: usize = 2;
/// Days a draft waits for its cause.
pub const DRAFT_DAYS: i64 = 14;

/// Open steps that several conversations handed off around without taking
/// or closing them: still wanted now? (`steps`: open ones, number and text.)
/// A step added within the log counts from then.
pub fn stuck_steps(f: &Facts) -> Vec<Hint> {
    let mut stuck: Vec<(usize, &str, usize)> = Vec::new();
    for (n, text) in &f.steps {
        // from when it was added, if the log has it
        let from = f
            .journal
            .iter()
            .rposition(|e| matches!(&e.did, Did::Step { text: t } if t == text))
            .map(|i| i + 1)
            .unwrap_or(0);
        let after = &f.journal[from.min(f.journal.len())..];
        if after.iter().any(|e| matches!(e.did, Did::Take { step } if step == *n)) {
            continue;
        }
        let finished: BTreeSet<&str> =
            after.iter().filter(|e| matches!(e.did, Did::Finish { .. })).map(|e| e.conversation.as_str()).collect();
        if finished.len() >= STUCK_AFTER {
            stuck.push((*n, text, finished.len()));
        }
    }
    if stuck.is_empty() {
        return Vec::new();
    }
    let shown: Vec<String> = stuck.iter().take(3).map(|(n, t, _)| format!("{n}. {}", one_line(t, 50))).collect();
    let numbers: Vec<String> = stuck.iter().map(|(n, _, _)| n.to_string()).collect();
    let most = stuck.iter().map(|s| s.2).max().unwrap_or(0);
    vec![Hint {
        key: format!("stuck:{}", numbers.join(",")),
        weight: 25,
        text: format!(
            "open through {most} handoffs, nobody took them: {}{} (still wanted? later: pg steps --defer {}; not wanted: pg steps --drop <number>)",
            shown.join("; "),
            if stuck.len() > 3 { format!(" +{}", stuck.len() - 3) } else { String::new() },
            numbers.join(",")
        ),
        why: format!("{STUCK_AFTER}+ conversations handed off without taking or closing them"),
    }]
}

/// Drafts pg wrote that nobody confirmed for `DRAFT_DAYS`: no longer
/// offered as a known fix; worth a cause, or taking out.
pub fn stale_drafts(f: &Facts) -> Vec<Hint> {
    let old: Vec<&str> =
        f.drafts.iter().filter(|(_, at)| f.now - at >= DRAFT_DAYS * 86_400).map(|(id, _)| id.as_str()).collect();
    if old.is_empty() {
        return Vec::new();
    }
    let shown: Vec<&str> = old.iter().take(6).copied().collect();
    vec![Hint {
        key: format!("drafts:{}", old.join(",")),
        weight: 20,
        text: format!(
            "{} pg wrote waited {DRAFT_DAYS}+ days for a cause: {}{} (worth keeping: pg write --error \"<its line>\" --cause \"...\"; not: pg rm <id>)",
            count(old.len() as i64, "draft"),
            shown.join(", "),
            if old.len() > 6 { format!(" +{}", old.len() - 6) } else { String::new() }
        ),
        why: "drafts unconfirmed for two weeks are no longer offered".into(),
    }]
}

/// Archived notes read in full again in `REVIVE_AFTER` conversations or
/// more since they were archived: worth bringing back.
pub fn archive_revival(f: &Facts) -> Vec<Hint> {
    let mut out = Vec::new();
    for (id, path) in &f.archived {
        // since it was archived, when the log has it: reads before say nothing
        let plain = path.strip_prefix(".archive/").unwrap_or(path);
        let since = f
            .journal
            .iter()
            .filter(|e| match &e.did {
                Did::Rm { what } => what == id || what == plain || what == path,
                Did::Mv { to, .. } => to == path || to.trim_end_matches(".md") == path.trim_end_matches(".md"),
                _ => false,
            })
            .map(|e| e.at)
            .max()
            .unwrap_or(i64::MIN);
        let readers: BTreeSet<&str> =
            f.reads.iter().filter(|(r, _, at)| r == id && *at > since).map(|(_, conv, _)| conv.as_str()).collect();
        if readers.len() < REVIVE_AFTER {
            continue;
        }
        let back = path.strip_prefix(".archive/").unwrap_or(path).trim_end_matches(".md");
        let how = if path.starts_with(".archive/") {
            format!("pg mv {} {back}", path.trim_end_matches(".md"))
        } else {
            format!("pg write --confirm {id}")
        };
        out.push(Hint {
            key: format!("revive:{id}"),
            weight: 22,
            text: format!("{id} is archived but was read again in {} conversations: back in use? {how}", readers.len()),
            why: "an archived note in use again".into(),
        });
    }
    out
}

/// A handoff that says what was done but not what is next, while steps
/// are open: the next conversation starts from it.
pub fn thin_handoff(summary: &str, open_steps: usize) -> Option<Hint> {
    if open_steps == 0 {
        return None;
    }
    let s = summary.to_lowercase();
    let next_words = [
        "next",
        "then",
        "todo",
        "to do",
        "remaining",
        "left",
        "still",
        "after",
        "later",
        "waiting",
        "open",
        "blocked",
        "sıradaki",
        "sonra",
        "kalan",
        "bekliyor",
    ];
    // "<what was done>; <what is next>", as pg finish asks, or a word for it
    let second_part = summary.split_once(';').is_some_and(|(_, next)| next.trim().chars().count() >= 3);
    let words: Vec<&str> = s.split(|c: char| !c.is_alphanumeric()).collect();
    let says_next =
        second_part || next_words.iter().any(|w| if w.contains(' ') { s.contains(w) } else { words.contains(w) });
    if says_next {
        return None;
    }
    Some(Hint {
        key: "thin-handoff".into(),
        weight: 50,
        text: format!(
            "the handoff says what was done, not what is next ({} open): pg finish again with \"<what was done>; next: <what>\" (it replaces this one)",
            count(open_steps as i64, "step")
        ),
        why: "a handoff without what is next, steps still open".into(),
    })
}

/// A rule of some code, as the rule guard reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub id: String,
    pub text: String,
    /// The folders and files it applies to (`(applies: payments/, a.go)`);
    /// empty: the whole project.
    pub applies: Vec<String>,
}

/// What a rule forbids, as words code would have: the backticked terms
/// after "never", "no", "not", "don't", "do not", "avoid" in it, and the
/// one word right after "never" or "no" when it looks like code (lowercase
/// letters, digits, `_` or `.`, not a common English word).
pub fn forbidden(rule: &str) -> Vec<String> {
    let lower = rule.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    let negations = ["never ", "no ", "not ", "don't ", "do not ", "avoid ", "without "];
    for neg in negations {
        let mut from = 0;
        while let Some(i) = lower[from..].find(neg) {
            let at = from + i;
            from = at + neg.len();
            // a word starting there, not inside another ("piano ")
            if at > 0 && lower[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric()) {
                continue;
            }
            let rest = &rule[at + neg.len()..];
            // backticked terms up to the end of the clause
            let clause: &str = rest.split(['.', ';']).next().unwrap_or(rest);
            let mut ticks = clause.split('`');
            ticks.next();
            let mut any = false;
            while let Some(term) = ticks.next() {
                let term = term.trim();
                if !term.is_empty() && !out.iter().any(|o| o == term) {
                    out.push(term.to_string());
                    any = true;
                }
                ticks.next();
            }
            if any || !(neg == "never " || neg == "no ") {
                continue;
            }
            let word: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.').collect();
            let word = word.trim_end_matches('.');
            let codey = word.len() >= 3
                && word.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
                && !ENGLISH.contains(&word);
            if codey && !out.iter().any(|o| o == word) {
                out.push(word.to_string());
            }
        }
    }
    out
}

/// Words after "never" or "no" that are English, not code.
const ENGLISH: &[&str] = &[
    "the",
    "one",
    "more",
    "less",
    "longer",
    "other",
    "matter",
    "need",
    "way",
    "way.",
    "change",
    "changes",
    "code",
    "file",
    "files",
    "test",
    "tests",
    "data",
    "user",
    "users",
    "network",
    "time",
    "again",
    "before",
    "after",
    "longer",
    "than",
    "any",
    "all",
    "such",
    "two",
    "new",
    "old",
    "direct",
    "manual",
    "secret",
    "secrets",
    "keys",
    "key",
    "values",
    "value",
    "notes",
    "note",
    "comments",
    "commit",
    "commits",
    "push",
    "merge",
    "release",
    "force",
    "edit",
    "edits",
    "touch",
    "shared",
    "global",
    "state",
    "magic",
    "hardcoded",
    "raw",
    "blocking",
];

/// Lines a change added (file, line, text) that hold what a rule of that
/// file forbids: "r3 says never float: payments/refund.go:41 adds float64".
pub fn rule_guard(rules: &[Rule], added: &[(String, usize, String)]) -> Vec<Hint> {
    let mut out = Vec::new();
    for r in rules {
        let terms = forbidden(&r.text);
        if terms.is_empty() {
            continue;
        }
        for (file, line, text) in added {
            if !r.applies.is_empty() && !r.applies.iter().any(|a| applies_to(a, file)) {
                continue;
            }
            let code = text.split("//").next().unwrap_or(text);
            let code = if code.trim_start().starts_with('#') { "" } else { code };
            let Some(term) = terms.iter().find(|t| has_term(code, t)) else { continue };
            out.push(Hint {
                key: format!("guard:{}:{file}:{line}", r.id),
                weight: 85,
                text: format!(
                    "{file}:{line} adds `{term}`; {} says: {} (on purpose? say why in the handoff)",
                    r.id,
                    one_line(&r.text, 120)
                ),
                why: format!("rule {} forbids {term:?} in this code", r.id),
            });
            break; // one line a rule: the first one shows it
        }
    }
    out
}

/// Whether a rule's `applies:` entry covers a file ("payments/" a folder,
/// "pricing/vat.go" a file), by the path's end.
fn applies_to(entry: &str, file: &str) -> bool {
    let e = entry.trim().trim_start_matches("./").replace('\\', "/");
    let f = file.replace('\\', "/");
    if e.ends_with('/') {
        f.starts_with(&e) || f.contains(&format!("/{e}"))
    } else {
        f == e || f.ends_with(&format!("/{e}"))
    }
}

/// A term as code: a backticked phrase as it is; a word at the start of an
/// identifier ("float" in float64, f.float, not in "afloat" or a string).
fn has_term(code: &str, term: &str) -> bool {
    let quotes_before = |i: usize| code[..i].chars().filter(|c| matches!(c, '"' | '\'' | '`')).count() % 2 == 1;
    code.match_indices(term).any(|(i, _)| {
        let before_ok = code[..i].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        before_ok && !quotes_before(i)
    })
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/kernel/watch.rs"]
mod tests;
