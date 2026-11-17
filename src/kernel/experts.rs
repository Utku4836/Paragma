//! The experts: small rules, each turning facts into hints. They read the
//! facts only; attention chooses among their hints.

use super::{Facts, Hint};
use crate::output::{ago, one_line};
use crate::state::journal::{Did, Entry};
use std::collections::HashMap;

/// A conversation quiet this long after changing something, with no
/// handoff, has stopped (cut, closed, out of context).
pub const QUIET: i64 = 20 * 60;

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
        let who = f.folders.get(&e.conversation).filter(|x| !x.is_empty()).map(String::as_str).unwrap_or("?");
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn entry(seq: i64, at: i64, conv: &str, did: Did) -> Entry {
        Entry { seq, at, conversation: conv.into(), did }
    }

    fn run(cmd: &str, failing: usize) -> Did {
        Did::Run { command: cmd.into(), passed: failing == 0, failing }
    }

    #[test]
    fn a_conversation_cut_short_is_told() {
        let w = |id: &str| Did::Write { kind: "rule".into(), id: id.into(), what: "created".into() };
        let f = Facts {
            now: 10_000,
            me: "me".into(),
            journal: vec![
                entry(1, 100, "a", Did::Open),
                entry(2, 110, "a", w("r1")),
                entry(3, 120, "a", Did::Finish { summary: "x".into() }),
                entry(4, 130, "a", w("r2")),
                entry(5, 140, "a", Did::Edit { file: "src/a.rs".into() }),
                entry(6, 150, "a", run("cargo test", 2)),
                entry(7, 160, "a", run("cargo test", 1)),
                entry(8, 170, "a", Did::Step { text: "s".into() }),
                // finished: nothing to tell
                entry(9, 200, "b", w("n1")),
                entry(10, 210, "b", Did::Finish { summary: "done".into() }),
                // only looked: nothing to tell
                entry(11, 220, "c", Did::Find { query: "q".into(), ids: vec![] }),
                // at work right now
                entry(12, 9_990, "d", w("d1")),
            ],
            folders: [("a".to_string(), "shop".to_string())].into(),
            ..Facts::default()
        };
        let hints = cut_short(&f);
        assert_eq!(hints.len(), 1, "{hints:?}");
        assert_eq!(
            hints[0].text,
            "shop stopped without a handoff (2 h ago): wrote r2; added 1 step; ran cargo test 2x (last: 1 failing); edited src/a.rs"
        );
        // compacted: its own work told to itself
        let mine = Facts { me: "a".into(), compacted: true, ..f };
        let h = cut_short(&mine);
        assert!(h.iter().any(|h| h.text.starts_with("this conversation before it was compacted")), "{h:?}");
    }

    #[test]
    fn recipes_are_the_commands_that_passed() {
        let r =
            |cmd: &str, exit: i32, at: i64| crate::testrun::Run { command: cmd.into(), exit, at, ..Default::default() };
        let f = Facts {
            runs: vec![
                r("cargo test --release", 0, 9),
                r("cargo test --release", 101, 8),
                r("cargo test", 101, 7),
                r("cargo build --release", 0, 6),
                r("python scripts/send.py", 0, 5),
                r("cargo clippy -- -D warnings", 1, 4),
            ],
            ..Facts::default()
        };
        let h = recipes(&f);
        assert_eq!(
            h[0].text,
            "how it runs here: tests: pg run -- cargo test --release (1x passed); build: pg run -- cargo build --release (1x passed)"
        );
        assert!(recipes(&Facts::default()).is_empty());
    }

    #[test]
    fn the_fix_is_what_changed_after_the_failure() {
        let at_fail = vec!["a.rs#parse=1".to_string(), "a.rs#render=2".to_string()];
        let now = vec!["a.rs#parse=1".to_string(), "a.rs#render=3".to_string(), "b.rs#new=4".to_string()];
        assert_eq!(fix_changes(Some(&at_fail), &now), Some(vec!["a.rs#render".into(), "b.rs#new".into()]));
        assert_eq!(fix_changes(None, &now), None);
    }

    proptest! {
        /// Any log: no panic, at most one hint per conversation, never about
        /// a conversation that handed off last or is still at work.
        #[test]
        fn any_log(raw in prop::collection::vec((0..4usize, 0..8usize, 0..20_000i64), 0..60), compacted: bool) {
            let convs = ["me", "a", "b", "c"];
            let mut journal: Vec<Entry> = raw.iter().enumerate().map(|(i, (c, k, at))| {
                let did = match k {
                    0 => Did::Open,
                    1 => Did::Write { kind: "rule".into(), id: format!("r{i}"), what: "created".into() },
                    2 => Did::Finish { summary: String::new() },
                    3 => Did::Edit { file: format!("f{i}.rs") },
                    4 => Did::Run { command: "t".into(), passed: i % 2 == 0, failing: i % 3 },
                    5 => Did::Step { text: "s".into() },
                    6 => Did::Rm { what: "x".into() },
                    _ => Did::Find { query: "q".into(), ids: vec![] },
                };
                entry(i as i64, *at, convs[*c], did)
            }).collect();
            journal.sort_by_key(|e| (e.at, e.seq));
            let f = Facts { now: 30_000, me: "me".into(), journal: journal.clone(), compacted, ..Facts::default() };
            let hints = cut_short(&f);
            for h in &hints {
                let conv = h.key.split(':').nth(1).unwrap_or("me");
                let mine: Vec<&Entry> = journal.iter().filter(|e| e.conversation == conv).collect();
                let finished = matches!(mine.last().map(|e| &e.did), Some(Did::Finish { .. }));
                prop_assert!(!finished, "told about a conversation that handed off last");
            }
            let mut keys: Vec<&str> = hints.iter().map(|h| h.key.split(':').nth(1).unwrap_or("")).collect();
            keys.dedup();
            prop_assert_eq!(keys.len(), hints.len());
        }
    }
}
