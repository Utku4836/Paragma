//! What pg does by itself: decision level 1. Small steps inside memory that
//! undo cleanly, each kept in the event log (`auto`) and listed by `pg
//! undo`, which takes any of them back:
//! - a handoff for a conversation that changed the project and stopped
//!   without one (written from what the log says it did);
//! - a draft record of an error a test run fixed: the error and the change
//!   that fixed it; the agent confirms it with the cause, or it fades.
//!
//! And what it says without writing: at `pg finish` without `--done`, the
//! steps this conversation took and left open (named in its handoff as
//! worked on, not closed: "half done" is not done); before an edit, that
//! another conversation still at work changed the same file.
//!
//! Never: deleting or moving a note the user or an agent wrote, changing
//! their text, touching code.

use crate::index::update_note;
use crate::kernel::Facts;
use crate::kernel::experts::{QUIET, stopped};
use crate::note::Note;
use crate::output::{ago, date_time, now, one_line};
use crate::project::{Project, plan};
use crate::state::State;
use crate::state::journal::Did;
use anyhow::Result;

/// A draft record not confirmed in this many days is no longer offered as
/// the known fix (it stays, marked, for `pg find`).
pub const DRAFT_DAYS: i64 = 14;

/// Writes the handoff of each conversation that stopped without one; a line
/// for each, to say in the brief.
pub fn handoffs_for_stopped(project: &Project, state: &State, facts: &Facts) -> Result<Vec<String>> {
    let mut said = Vec::new();
    for s in stopped(facts) {
        let key = format!("auto-handoff:{}", s.conversation);
        if state.get(&key).and_then(|v| v.parse::<i64>().ok()).is_some_and(|seq| seq >= s.last_seq) {
            continue; // written already for what it did
        }
        let mine = format!("handoff:{}", s.conversation);
        let earlier = state.get(&mine).filter(|h| !h.is_empty());
        let heading = if s.folder.is_empty() {
            format!("{} (written by pg)", date_time(s.last_at))
        } else {
            format!("{} · {} (written by pg)", date_time(s.last_at), s.folder)
        };
        let text = format!("Stopped without a handoff; from what it did: {}", s.what);
        update_note(project, plan::FILE, |n, exists, _| {
            if !exists {
                n.props.set_text("type", crate::note::Kind::Project.as_str());
                n.props.set_text("title", project.name.clone());
            }
            // only one pg wrote for it: its own handoff, an older one of pg's
            // for it, is replaced
            let replaces = earlier.as_deref().filter(|h| h.ends_with("(written by pg)"));
            // two pg opening at once write it once
            plan::remove_handoff(n, &heading);
            plan::add_handoff(n, &heading, &text, replaces);
            Ok(())
        })?;
        // its own pg finish, if it comes back, replaces this one
        state.set(&mine, &heading)?;
        state.set(&key, &s.last_seq.to_string())?;
        let seq = state.record_numbered(Did::Auto {
            what: "handoff".into(),
            target: s.conversation.clone(),
            detail: heading.clone(),
        })?;
        // the brief says it here, not again as a hint
        state.say(&[format!("cut:{}:{}", s.conversation, s.last_seq)])?;
        let who = if s.folder.is_empty() { "a conversation".to_string() } else { s.folder.clone() };
        said.push(format!(
            "{who} stopped without a handoff ({}); pg wrote one from what it did: {} (pg undo {seq})",
            ago(s.last_at, now()),
            s.what
        ));
    }
    Ok(said)
}

/// Writes a draft record of a fixed error: the error, and the change that
/// fixed it. The line to say, or None when memory has it already.
pub fn draft_fix(
    project: &Project,
    state: &State,
    line: &str,
    changes: &[String],
    files: &[String],
) -> Result<Option<String>> {
    if changes.is_empty() {
        return Ok(None);
    }
    let e = crate::record::ErrorInput {
        line,
        cause: None,
        fix: Some(&format!("Changed: {}.", changes.join(", "))),
        files: files.to_vec(),
        symbols: Vec::new(),
        commit: None,
        code_state: None,
        draft: true,
    };
    let w = crate::record::error(project, &e)?;
    if w.what != "created" {
        return Ok(None); // it was there (written meanwhile): nothing drafted
    }
    let Some(row) = crate::index::Index::open(project)?.note_by_ref(&w.id)? else { return Ok(None) };
    let rel = row.path;
    update_note(project, &rel, |n, _, _| {
        n.props.set_text("status", "draft");
        n.set_section("Cause", "(draft: not said yet)");
        Ok(())
    })?;
    let seq = state.record_numbered(Did::Auto { what: "draft".into(), target: w.id.clone(), detail: rel })?;
    Ok(Some(format!(
        "pg wrote {}, a draft of this fix (the error and the change that fixed it). Say why, and it is kept: \
         pg write --error {:?} --cause \"...\" (--fix to change the fix); not worth keeping: pg undo {seq}",
        w.id,
        one_line(line, 120)
    )))
}

/// The section of rules.md pg's draft rules go to.
pub const DRAFT_RULES: &str = "Drafts (written by pg)";

/// A known error shown in several conversations (kernel/learn.rs): a draft
/// rule in rules.md, once an error; or, while the gate keeps it to saying,
/// the line that proposes it. Lines to say in the brief.
pub fn rule_drafts(project: &Project, state: &State, facts: &Facts) -> Result<Vec<String>> {
    use crate::kernel::learn;
    let rules_path = project.note_file(crate::record::RULES_FILE);
    let rules = crate::project::files::read_text(&rules_path).unwrap_or_default();
    // the errors a rule names already ("from e12")
    let cited: Vec<String> = rules
        .split("from ")
        .skip(1)
        .filter_map(|r| {
            let id: String = r.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            (id.starts_with('e') && id.len() > 1).then_some(id)
        })
        .collect();
    let mut said = Vec::new();
    let index = crate::index::Index::open(project)?;
    for c in learn::rule_candidates(facts, &cited) {
        let key = format!("rule-draft:{}", c.error);
        if state.get(&key).is_some() {
            continue; // written (or taken back) once: not again
        }
        let Some(row) = index.note_by_ref(&c.error)? else { continue };
        let note = Note::parse(&crate::project::files::read_text(&project.dir.join(&row.path)).unwrap_or_default());
        if note.props.text("status") == Some("draft") {
            continue; // a draft is no ground for a rule
        }
        let title = note.props.text("title").unwrap_or(&c.error).to_string();
        let fix = note.section("Fix").unwrap_or_default();
        let text = learn::rule_text(&c, &title, &fix);
        if !crate::kernel::gate::may_write(facts, "rule-draft") {
            if !state.said()?.contains(&key) {
                state.say(std::slice::from_ref(&key))?;
                said.push(format!(
                    "{} keeps coming back ({} conversations): worth a rule? pg write --rule \"...\" (from {})",
                    c.error, c.conversations, c.error
                ));
            }
            continue;
        }
        let (_, id) = update_note(project, crate::record::RULES_FILE, |n, exists, index| {
            if !exists {
                n.props.set_text("type", crate::note::Kind::Rule.as_str());
                n.props.set_text("title", "Rules");
            }
            let id = index.next_id(crate::note::Kind::Rule)?;
            let section = n.section(DRAFT_RULES).unwrap_or_default();
            let line = format!("- {id}: {text}");
            n.set_section(DRAFT_RULES, &if section.is_empty() { line } else { format!("{section}\n{line}") });
            Ok(id)
        })?;
        state.set(&key, &id)?;
        let seq = state.record_numbered(Did::Auto {
            what: "rule-draft".into(),
            target: id.clone(),
            detail: c.error.clone(),
        })?;
        said.push(format!(
            "{} came back in {} conversations: pg wrote {id}, a draft rule from its fix (not binding until confirmed: pg write --confirm {id}; reworded: pg write --rule \"...\"; not worth it: pg undo {seq})",
            c.error, c.conversations
        ));
    }
    Ok(said)
}

/// Tests that failed and passed with the same code (kernel/learn.rs): a
/// draft record for each, once; or the line saying it, while the gate keeps
/// the expert to saying. Lines to say after the run.
pub fn flaky_records(project: &Project, state: &State, facts: &Facts, command: &str) -> Result<Vec<String>> {
    use crate::kernel::learn;
    let runs = state.runs(None, 200)?;
    let command = crate::testrun::normalized(command);
    let mut said = Vec::new();
    for f in learn::flaky(&runs).into_iter().filter(|f| f.command == command) {
        let key = format!("flaky:{}:{}", f.command, f.test);
        if state.get(&key).is_some() {
            continue;
        }
        let line = learn::flaky_line(&f);
        let index = crate::index::Index::open(project)?;
        if crate::record::known_error(&index, &line)?.is_some() {
            state.set(&key, "known")?;
            continue; // memory has it already
        }
        if !crate::kernel::gate::may_write(facts, "flaky") {
            state.set(&key, "said")?;
            said.push(format!(
                "{} failed {} of the last {} runs, and passed with the same code: flaky. Worth a record: pg write --error {:?} --cause \"...\"",
                f.test, f.fails, f.runs, line
            ));
            continue;
        }
        let cause = format!(
            "(draft: failed {} of the last {} runs and passed with the same code: by chance, not by a change)",
            f.fails, f.runs
        );
        let e = crate::record::ErrorInput {
            line: &line,
            cause: Some(&cause),
            fix: None,
            files: Vec::new(),
            symbols: Vec::new(),
            commit: None,
            code_state: None,
            draft: true,
        };
        let w = crate::record::error(project, &e)?;
        if w.what != "created" {
            state.set(&key, "known")?;
            continue;
        }
        let Some(row) = index.note_by_ref(&w.id)? else { continue };
        update_note(project, &row.path, |n, _, _| {
            n.props.set_text("status", "draft");
            Ok(())
        })?;
        state.set(&key, &w.id)?;
        let seq = state.record_numbered(Did::Auto { what: "flaky".into(), target: w.id.clone(), detail: row.path })?;
        said.push(format!(
            "{} failed {} of the last {} runs and passed with the same code: pg wrote {}, a draft record of it as flaky (why it fails: pg write --error {:?} --cause \"...\"; not flaky: pg undo {seq})",
            f.test, f.fails, f.runs, w.id, line
        ));
    }
    Ok(said)
}

/// At a handoff, the fixes shown whose tests still failed (state/outcome.rs):
/// a dated line on each record, so every computer knows the fix did not
/// help this time.
pub fn outcome_lines(
    project: &Project,
    state: &State,
    facts: &Facts,
    missed: &[(String, String, Vec<String>)],
) -> Result<()> {
    if !crate::kernel::gate::may_write(facts, "outcome") {
        return Ok(()); // the known fix says it from this computer's outcomes
    }
    let index = crate::index::Index::open(project)?;
    for (id, command, tests) in missed {
        let Some(row) = index.note_by_ref(id)? else { continue };
        let line = crate::kernel::learn::outcome_line(&crate::output::date(now()), command, tests);
        update_note(project, &row.path, |n, _, _| {
            let section = n.section("Outcomes").unwrap_or_default();
            n.set_section("Outcomes", &if section.is_empty() { line.clone() } else { format!("{section}\n{line}") });
            Ok(())
        })?;
        state.record_numbered(Did::Auto {
            what: "outcome".into(),
            target: id.clone(),
            detail: format!("{}\t{line}", row.path),
        })?;
    }
    Ok(())
}

/// The commands that passed here, kept in the project note's "How it
/// runs" (state.db is not synced: another computer reads them there). Not
/// when someone changed what pg wrote: then theirs stays.
pub fn keep_recipes(project: &Project, state: &State, facts: &Facts) -> Result<()> {
    use crate::kernel::learn::{RECIPE_SECTION, recipe_section};
    let Some(text) = recipe_section(facts) else { return Ok(()) };
    if !crate::kernel::gate::may_write(facts, "recipe") {
        return Ok(()); // the brief says them from the runs
    }
    let path = project.note_file(plan::FILE);
    let note = crate::project::files::read_text(&path).map(|t| Note::parse(&t)).unwrap_or_default();
    let now_there = note.section(RECIPE_SECTION);
    let mine = state.get("recipe:written");
    match &now_there {
        Some(t) if t.trim() == text.trim() => return Ok(()),
        // someone wrote it or changed what pg wrote: theirs
        Some(t) if mine.as_deref().map(str::trim) != Some(t.trim()) => return Ok(()),
        _ => {}
    }
    update_note(project, plan::FILE, |n, exists, _| {
        if !exists {
            n.props.set_text("type", crate::note::Kind::Project.as_str());
            n.props.set_text("title", project.name.clone());
        }
        n.set_section(RECIPE_SECTION, &text);
        Ok(())
    })?;
    state.set("recipe:written", &text)?;
    state.record_numbered(Did::Auto {
        what: "recipe".into(),
        target: plan::FILE.into(),
        detail: now_there.unwrap_or_default().replace('\n', "\u{1f}"),
    })?;
    Ok(())
}

/// What a change added that a rule of that code forbids (kernel/watch.rs):
/// the rules with what they apply to, the lines the change added. Draft
/// rules are not rules yet.
pub fn guarded(
    project: &Project,
    code: &crate::code::CodeIndex,
    only: Option<&[String]>,
) -> Result<Vec<crate::kernel::Hint>> {
    use crate::kernel::watch::{Rule, rule_guard};
    if !crate::kernel::gate::may_say("guard") {
        return Ok(Vec::new());
    }
    let index = crate::index::Index::open(project)?;
    let rules: Vec<Rule> = index
        .pieces_of(crate::note::Kind::Rule)?
        .into_iter()
        .filter_map(|p| {
            let text = crate::note::rule_line(&p.text).map(|(_, t)| t.to_string()).unwrap_or(p.text.clone());
            (!text.starts_with("(draft)")).then(|| Rule {
                applies: crate::code::notes::applies(&text).unwrap_or_default(),
                text: text.split("(applies:").next().unwrap_or(&text).trim().to_string(),
                id: p.id,
            })
        })
        .collect();
    if rules.iter().all(|r| crate::kernel::watch::forbidden(&r.text).is_empty()) {
        return Ok(Vec::new());
    }
    let mut added: Vec<(String, usize, String)> = Vec::new();
    for f in &code.folders {
        for (path, ranges) in crate::code::git::changed(&f.dir) {
            if only.is_some_and(|o| !o.iter().any(|x| x.replace('\\', "/").ends_with(&path))) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(f.dir.join(&path)) else { continue };
            let lines: Vec<&str> = text.lines().collect();
            let ranges = ranges.unwrap_or_else(|| vec![(1, lines.len())]);
            for (a, b) in ranges {
                for n in a..=b.min(lines.len()) {
                    if let Some(l) = lines.get(n.wrapping_sub(1)) {
                        added.push((path.clone(), n, l.to_string()));
                    }
                }
            }
        }
    }
    Ok(rule_guard(&rules, &added))
}

/// Whether a record is a draft, and still offered (younger than
/// `DRAFT_DAYS`).
pub fn draft_state(note: &Note) -> Option<bool> {
    if note.props.text("status") != Some("draft") {
        return None;
    }
    let seen = note.props.text("last_seen").and_then(crate::output::parse_date);
    Some(seen.is_none_or(|t| now() - t < DRAFT_DAYS * 86_400))
}

/// The steps this conversation took (`pg steps --take`) since `since` that
/// are still open, with their texts: at `pg finish` without `--done`, named
/// in its handoff as worked on and asked about.
pub fn taken_open(project: &Project, state: &State, since: i64) -> Result<Vec<(usize, String)>> {
    let mine = crate::state::conversation::key();
    let mut taken: Vec<usize> = Vec::new();
    for e in state.journal_of("take", since)?.into_iter().filter(|e| e.conversation == mine) {
        if let Did::Take { step } = e.did
            && !taken.contains(&step)
        {
            taken.push(step);
        }
    }
    if taken.is_empty() {
        return Ok(Vec::new());
    }
    let note =
        crate::project::files::read_text(&project.note_file(plan::FILE)).map(|t| Note::parse(&t)).unwrap_or_default();
    let steps = plan::steps(&note);
    Ok(taken
        .into_iter()
        .filter_map(|k| steps.get(k.checked_sub(1)?).filter(|s| s.open()).map(|s| (k, s.text.clone())))
        .collect())
}

/// Another conversation, still at work, changed one of these files (paths
/// as the hooks keep them): a key for each (said once) and who and when, to
/// say before an edit there.
pub fn others_editing(state: &State, files: &[String]) -> Result<Vec<(String, String)>> {
    let me = crate::state::conversation::key();
    let since = now() - QUIET;
    let journal = state.journal(since)?;
    let folders = state.folders(since)?;
    let mut out = Vec::new();
    for f in files {
        let norm = |p: &str| p.replace('\\', "/").to_lowercase();
        let hit = journal.iter().rev().find(|e| {
            e.conversation != me
                && matches!(&e.did, Did::Edit { file } if norm(file) == norm(f) || norm(file).ends_with(&format!("/{}", norm(f))) || norm(f).ends_with(&format!("/{}", norm(file))))
        });
        let Some(e) = hit else { continue };
        // gone since: it handed off
        if journal
            .iter()
            .any(|x| x.conversation == e.conversation && x.seq > e.seq && matches!(x.did, Did::Finish { .. }))
        {
            continue;
        }
        let who = folders
            .get(&e.conversation)
            .filter(|x| !x.is_empty())
            .cloned()
            .unwrap_or_else(|| "another conversation".into());
        let name = f.rsplit(['/', '\\']).next().unwrap_or(f);
        out.push((
            format!("conflict:{}:{name}", e.conversation),
            format!("{who} changed {name} {} and is still at work", ago(e.at, now())),
        ));
    }
    Ok(out)
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/cli/act.rs"]
mod tests;
