//! The experts that only say (decision level 0): a hint with how to act on
//! it, never an action. Each is measured on the scenario sets; one that
//! misfires is turned off (kernel/gate.rs).
//! - stuck step: open while several conversations handed off without
//!   touching it;
//! - stale drafts: drafts pg wrote that nobody confirmed in two weeks;
//! - archive revival: a note pg archived (unused for long) read again in
//!   several conversations;
//! - thin handoff: a handoff without the part that says what is next.

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
        out.push(Hint {
            key: format!("revive:{id}"),
            weight: 22,
            text: format!(
                "{id} was archived (unused for long) but was read again in {} conversations: back in use? pg write --confirm {id}",
                readers.len()
            ),
            why: "an archived note in use again".into(),
        });
    }
    out
}

/// A handoff without what is next, while steps are open: the next
/// conversation starts from it. Told by the form `pg finish` asks for,
/// "<what was done>; <what is next>", not by words of a language.
pub fn thin_handoff(summary: &str, open_steps: usize) -> Option<Hint> {
    if open_steps == 0 {
        return None;
    }
    let next = summary.split_once(';').map(|(_, n)| n.trim()).unwrap_or("");
    if next.chars().filter(|c| c.is_alphanumeric()).count() >= 2 {
        return None;
    }
    Some(Hint {
        key: "thin-handoff".into(),
        weight: 50,
        text: format!(
            "the handoff says what was done, not what is next ({} open): pg finish again with \"<what was done>; <what is next>\" (it replaces this one)",
            count(open_steps as i64, "step")
        ),
        why: "a handoff without its part after ';', steps still open".into(),
    })
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/kernel/watch.rs"]
mod tests;
