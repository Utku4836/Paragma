//! The experts: small rules, each turning facts into hints. They read the
//! facts only; attention chooses among their hints.

use super::{Facts, Hint};
use crate::output::{ago, one_line};
use crate::state::journal::{Did, Entry};
use std::collections::HashMap;

/// A conversation quiet this long after changing something, with no
/// handoff, has stopped (cut, closed, out of context).
pub const QUIET: i64 = 20 * 60;

/// Another conversation that changed the project and stopped without a
/// handoff: what it did since its last one.
pub struct Stopped {
    pub conversation: String,
    /// Its folder's name ("" when not known).
    pub folder: String,
    /// Its last event.
    pub last_seq: i64,
    pub last_at: i64,
    /// "wrote r3; added 2 steps; ran cargo test 4x (last: 1 failing)"
    pub what: String,
}

/// The other conversations that stopped without a handoff (quiet for
/// `QUIET` after changing the project). pg writes their handoff for them
/// (cli/act.rs); `cut_short` says it.
pub fn stopped(f: &Facts) -> Vec<Stopped> {
    let mut by: HashMap<&str, Vec<&Entry>> = HashMap::new();
    for e in &f.journal {
        by.entry(e.conversation.as_str()).or_default().push(e);
    }
    let mut convs: Vec<&&str> = by.keys().filter(|c| ***c != f.me).collect();
    convs.sort();
    let mut out = Vec::new();
    for conv in convs {
        let all = &by[*conv];
        let after = match all.iter().rposition(|e| matches!(e.did, Did::Finish { .. })) {
            Some(i) => &all[i + 1..],
            None => &all[..],
        };
        if !after.iter().any(|e| e.did.changes()) {
            continue;
        }
        let Some(last) = after.last() else { continue };
        if f.now - last.at < QUIET {
            continue;
        }
        let what = summary(after);
        if what.is_empty() {
            continue;
        }
        out.push(Stopped {
            conversation: conv.to_string(),
            folder: f.folders.get(*conv).cloned().unwrap_or_default(),
            last_seq: last.seq,
            last_at: last.at,
            what,
        });
    }
    out
}

/// Conversations that changed the project and stopped without `pg finish`,
/// told from the event log; and this conversation's own work when it was
/// compacted. Nothing is lost when a conversation ends without a handoff.
pub fn cut_short(f: &Facts) -> Vec<Hint> {
    let mut by: HashMap<&str, Vec<&Entry>> = HashMap::new();
    for e in &f.journal {
        by.entry(e.conversation.as_str()).or_default().push(e);
    }
    let mut out = Vec::new();
    let mut convs: Vec<&&str> = by.keys().collect();
    convs.sort();
    for conv in convs {
        let all = &by[*conv];
        // what it did since its last handoff
        let after = match all.iter().rposition(|e| matches!(e.did, Did::Finish { .. })) {
            Some(i) => &all[i + 1..],
            None => &all[..],
        };
        if !after.iter().any(|e| e.did.changes()) {
            continue;
        }
        let Some(last) = after.last() else { continue };
        let what = summary(after);
        if what.is_empty() {
            continue;
        }
        if *conv == f.me {
            if f.compacted {
                out.push(Hint {
                    key: format!("so-far:{conv}:{}", last.seq),
                    weight: 90,
                    text: format!("this conversation before it was compacted (no handoff yet): {what}"),
                    why: "compacted: what it did may be gone from its context".into(),
                });
            }
            continue;
        }
        if f.now - last.at < QUIET {
            continue; // still at work: the brief lists it as such
        }
        let who = f.folders.get(*conv).filter(|x| !x.is_empty()).map(String::as_str).unwrap_or("a conversation");
        out.push(Hint {
            key: format!("cut:{conv}:{}", last.seq),
            weight: 80,
            text: format!("{who} stopped without a handoff ({}): {what}", ago(last.at, f.now)),
            why: "changed the project, then nothing, and no pg finish".into(),
        });
    }
    out
}

/// The other conversations working here now (busy within `QUIET`, no
/// handoff yet): two at once may step on each other.
pub fn at_work(f: &Facts) -> Vec<Hint> {
    let mut last: HashMap<&str, &Entry> = HashMap::new();
    for e in &f.journal {
        last.insert(e.conversation.as_str(), e);
    }
    let mut busy: Vec<&&Entry> = last
        .iter()
        .filter(|(c, e)| **c != f.me && f.now - e.at < QUIET && !matches!(e.did, Did::Finish { .. }))
        .map(|(_, e)| e)
        .collect();
    if busy.is_empty() {
        return Vec::new();
    }
    busy.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.conversation.cmp(&b.conversation)));
    // by folder: "shop ×2 (just now)"
    let mut by_folder: Vec<(&str, usize, i64)> = Vec::new();
    for e in &busy {
        let who =
            f.folders.get(&e.conversation).filter(|x| !x.is_empty()).map(String::as_str).unwrap_or("a conversation");
        match by_folder.iter_mut().find(|b| b.0 == who) {
            Some(b) => b.1 += 1,
            None => by_folder.push((who, 1, e.at)),
        }
    }
    let names: Vec<String> = by_folder
        .iter()
        .take(3)
        .map(|(who, n, at)| {
            let times = if *n > 1 { format!(" ×{n}") } else { String::new() };
            format!("{who}{times} ({})", ago(*at, f.now))
        })
        .collect();
    let key = busy.iter().map(|e| format!("{}:{}", e.conversation, e.seq)).collect::<Vec<_>>().join(",");
    vec![Hint {
        key: format!("work:{key}"),
        weight: 85,
        text: format!("other conversations at work, not finished: {}", names.join(", ")),
        why: "busy here in the last minutes, no handoff".into(),
    }]
}

/// A step taken lasts while its conversation works: until its handoff, or
/// this long quiet.
pub const HOLD: i64 = 60 * 60;
/// A note between conversations is news this long.
pub const NEWS: i64 = 24 * 60 * 60;

/// Who has taken a step.
#[derive(Clone, Debug, PartialEq)]
pub struct Claim {
    pub conversation: String,
    /// When it was taken.
    pub at: i64,
}

/// The steps taken now, by number: the last take of each, while the
/// conversation that took it has not handed off and is not quiet for `HOLD`.
pub fn claims(f: &Facts) -> std::collections::BTreeMap<usize, Claim> {
    let mut last: HashMap<&str, (i64, i64)> = HashMap::new(); // conversation -> (last event, last finish)
    let mut taken: std::collections::BTreeMap<usize, Claim> = std::collections::BTreeMap::new();
    for e in &f.journal {
        let l = last.entry(e.conversation.as_str()).or_insert((0, i64::MIN));
        l.0 = l.0.max(e.at);
        match &e.did {
            Did::Finish { .. } => l.1 = e.at,
            Did::Take { step } if *step > 0 => {
                taken.insert(*step, Claim { conversation: e.conversation.clone(), at: e.at });
            }
            _ => {}
        }
    }
    taken.retain(|_, c| {
        let (seen, finished) = last.get(c.conversation.as_str()).copied().unwrap_or((0, i64::MIN));
        finished < c.at && f.now - seen < HOLD
    });
    taken
}

/// "(taken by shop, 5 min ago)" or "(yours)" for a step, when taken.
pub fn claim_mark(f: &Facts, claims: &std::collections::BTreeMap<usize, Claim>, step: usize) -> String {
    match claims.get(&step) {
        Some(c) if c.conversation == f.me => " (yours)".to_string(),
        Some(c) => {
            let who = f
                .folders
                .get(&c.conversation)
                .filter(|x| !x.is_empty())
                .map(String::as_str)
                .unwrap_or("another conversation");
            format!(" (taken by {who}, {})", ago(c.at, f.now))
        }
        None => String::new(),
    }
}

/// Notes the other conversations left here (pg tell) in the last day, each
/// said once.
pub fn told(f: &Facts) -> Vec<Hint> {
    f.journal
        .iter()
        .filter(|e| e.conversation != f.me && f.now - e.at < NEWS)
        .filter_map(|e| match &e.did {
            Did::Told { text } => {
                let who = f
                    .folders
                    .get(&e.conversation)
                    .filter(|x| !x.is_empty())
                    .map(String::as_str)
                    .unwrap_or("another conversation");
                Some(Hint {
                    key: format!("told:{}", e.seq),
                    weight: 95,
                    text: format!("from {who} ({}): {}", ago(e.at, f.now), one_line(text, 200)),
                    why: "another conversation here left it for the others".into(),
                })
            }
            _ => None,
        })
        .collect()
}

/// What a stretch of the log did, in one line: "wrote r3, n5; added 2
/// steps; ran cargo test 4x (last: 2 failing); edited src/a.rs, src/b.rs".
pub fn summary(entries: &[&Entry]) -> String {
    let mut written: Vec<&str> = Vec::new();
    let mut steps = 0;
    let mut runs: Vec<(&str, usize, &Did)> = Vec::new(); // command, times, last
    let mut edits: Vec<&str> = Vec::new();
    let mut moved = 0;
    let mut removed = 0;
    for e in entries {
        match &e.did {
            Did::Write { id, .. } if !id.is_empty() && !written.contains(&id.as_str()) => written.push(id),
            Did::Step { .. } => steps += 1,
            Did::Run { command, .. } => match runs.iter_mut().find(|r| r.0 == command) {
                Some(r) => {
                    r.1 += 1;
                    r.2 = &e.did;
                }
                None => runs.push((command, 1, &e.did)),
            },
            Did::Edit { file } if !edits.contains(&file.as_str()) => edits.push(file),
            Did::Mv { .. } => moved += 1,
            Did::Rm { .. } => removed += 1,
            _ => {}
        }
    }
    let mut parts: Vec<String> = Vec::new();
    if !written.is_empty() {
        let more = written.len().saturating_sub(8);
        let ids = written.iter().take(8).copied().collect::<Vec<_>>().join(", ");
        parts.push(format!("wrote {ids}{}", if more > 0 { format!(" (+{more})") } else { String::new() }));
    }
    if steps > 0 {
        parts.push(format!("added {}", crate::output::count(steps, "step")));
    }
    for (command, times, last) in runs.iter().rev().take(2) {
        let result = match last {
            Did::Run { passed: true, .. } => "last passed".to_string(),
            Did::Run { failing: n, .. } if *n > 0 => format!("last: {n} failing"),
            _ => "last failed".to_string(),
        };
        let times = if *times > 1 { format!(" {times}x") } else { String::new() };
        parts.push(format!("ran {}{times} ({result})", one_line(command, 50)));
    }
    if !edits.is_empty() {
        let more = edits.len().saturating_sub(5);
        let shown = edits.iter().take(5).copied().collect::<Vec<_>>().join(", ");
        parts.push(format!("edited {shown}{}", if more > 0 { format!(" (+{more})") } else { String::new() }));
    }
    if moved + removed > 0 {
        parts.push(format!("moved or removed {}", crate::output::count((moved + removed) as i64, "note")));
    }
    parts.join("; ")
}

/// What a command is for, from its words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Purpose {
    Tests,
    Build,
    Check,
    Other,
}

pub fn purpose(command: &str) -> Purpose {
    let c = command.to_lowercase();
    let words: Vec<&str> = c.split(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_').collect();
    let has = |w: &[&str]| words.iter().any(|x| w.contains(x));
    if has(&["test", "tests", "pytest", "jest", "vitest", "mocha", "rspec", "phpunit", "ctest", "nextest"]) {
        Purpose::Tests
    } else if has(&[
        "clippy", "lint", "eslint", "ruff", "flake8", "mypy", "fmt", "prettier", "black", "vet", "tsc", "check",
    ]) {
        Purpose::Check
    } else if has(&["build", "compile", "make", "cmake", "msbuild", "gradle", "webpack", "vite", "bundle"]) {
        Purpose::Build
    } else {
        Purpose::Other
    }
}

/// The commands that work here, learned from `pg run`: for tests, a build
/// and checks, the one used most that passed. The next conversation runs
/// them as they are, without searching for them.
pub fn recipes(f: &Facts) -> Vec<Hint> {
    /// A command as it was used here.
    struct Used<'a> {
        command: &'a str,
        passes: usize,
        newest: i64,
        /// It ran tests (its output had them), whatever its name.
        tests: bool,
    }
    let mut seen: Vec<Used> = Vec::new();
    for r in &f.runs {
        let tests = r.passed > 0 || !r.failing.is_empty();
        match seen.iter_mut().find(|s| s.command == r.command) {
            Some(s) => {
                s.passes += usize::from(r.exit == 0);
                s.tests |= tests;
            }
            None => seen.push(Used { command: &r.command, passes: usize::from(r.exit == 0), newest: r.at, tests }),
        }
    }
    let mut best: Vec<(Purpose, &Used)> = Vec::new();
    for s in seen.iter().filter(|s| s.passes > 0) {
        // what it did tells more than its name: a script that ran tests runs the tests
        let p = if s.tests { Purpose::Tests } else { purpose(s.command) };
        if p == Purpose::Other {
            continue; // a script or an API call is not a way to run the project
        }
        match best.iter_mut().find(|b| b.0 == p) {
            Some(b) if (s.passes, s.newest) > (b.1.passes, b.1.newest) => b.1 = s,
            Some(_) => {}
            None => best.push((p, s)),
        }
    }
    if best.is_empty() {
        return Vec::new();
    }
    best.sort_by_key(|b| b.0);
    let name = |p: Purpose| match p {
        Purpose::Tests => "tests",
        Purpose::Build => "build",
        Purpose::Check => "checks",
        Purpose::Other => "other",
    };
    let parts: Vec<String> = best
        .iter()
        .map(|(p, s)| format!("{}: pg run -- {} ({}x passed)", name(*p), one_line(s.command, 80), s.passes))
        .collect();
    vec![Hint {
        key: "recipes".into(),
        weight: 40,
        text: format!("how it runs here: {}", parts.join("; ")),
        why: "commands that passed here before".into(),
    }]
}

/// Notes whose code is gone or changed its signature (the last sleep
/// looked): named once, the reasons in pg doctor and pg read.
pub fn outdated(f: &Facts) -> Vec<Hint> {
    if f.outdated.is_empty() {
        return Vec::new();
    }
    let ids: Vec<&str> = f.outdated.iter().take(6).map(|(i, _)| i.as_str()).collect();
    let more = f.outdated.len().saturating_sub(6);
    vec![Hint {
        key: format!("outdated:{}", f.outdated.iter().map(|(i, _)| i.as_str()).collect::<Vec<_>>().join(",")),
        weight: 60,
        text: format!(
            "may be outdated (their code is gone or its signature changed): {}{} (pg read <id> says why; still right: pg write --confirm <id>)",
            ids.join(", "),
            if more > 0 { format!(" +{more}") } else { String::new() }
        ),
        why: "the code a note names changed in a way that matters".into(),
    }]
}

/// What could be tidier (the last sleep looked): one line, the list in pg
/// doctor.
pub fn tidier(f: &Facts) -> Vec<Hint> {
    let Some(first) = f.tidy.first() else { return Vec::new() };
    let more = f.tidy.len() - 1;
    vec![Hint {
        key: format!("tidy:{}", f.tidy.len()),
        weight: 30,
        text: format!(
            "the notes could be tidier: {first}{}",
            if more > 0 { format!(" (+{more} more: pg doctor)") } else { String::new() }
        ),
        why: "notes nearly the same, a note on several topics, or a folder with one note".into(),
    }]
}

/// The change that fixed a failing run: of the functions changed now
/// ("path#Name=hash"), those that were not changed, or changed otherwise,
/// when it failed. None when the failing run did not keep its functions
/// (an older pg): then nothing narrows it.
pub fn fix_changes(at_fail: Option<&[String]>, now: &[String]) -> Option<Vec<String>> {
    let at_fail = at_fail?;
    Some(
        now.iter()
            .filter(|s| !at_fail.contains(s))
            .map(|s| s.rsplit_once('=').map(|(name, _)| name).unwrap_or(s).to_string())
            .collect(),
    )
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/kernel/experts.rs"]
mod tests;
