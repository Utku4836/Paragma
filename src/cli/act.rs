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
