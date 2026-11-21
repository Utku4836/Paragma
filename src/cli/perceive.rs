//! Perception: the thin shell around the kernel. It reads what the kernel
//! decides from (the event log, the runs, what was already said) and does
//! nothing else; the kernel decides, the command prints.

use crate::kernel::Facts;
use crate::output::now;
use crate::project::Project;
use crate::state::{State, conversation};
use anyhow::Result;

/// How far back the log is read: a conversation quiet longer than this is
/// not news any more.
const LOOK_BACK: i64 = 14 * 86_400;
/// Runs read for what works here.
const RUNS: usize = 300;

/// Notes from the other conversations not said to this one yet, as lines;
/// remembered as said. Cheap: only the notes of the last day are read.
pub fn news(state: &State) -> Result<Vec<String>> {
    let now = now();
    let f = Facts {
        now,
        me: conversation::key().to_string(),
        journal: state.journal_of("tell", now - crate::kernel::experts::NEWS)?,
        folders: state.folders(now - LOOK_BACK)?,
        said: state.said()?,
        ..Facts::default()
    };
    let hints = crate::kernel::news_hints(&f, 200);
    state.say(&hints.iter().map(|h| h.key.clone()).collect::<Vec<_>>())?;
    Ok(hints.into_iter().map(|h| h.text).collect())
}

pub fn facts(project: &Project, state: &State, compacted: bool) -> Result<Facts> {
    let now = now();
    crate::output::mark("perceive: start");
    let found = super::sleep::found(state);
    let note = crate::project::files::read_text(&project.note_file(crate::project::plan::FILE))
        .map(|t| crate::note::Note::parse(&t))
        .unwrap_or_default();
    let steps: Vec<(usize, String)> = crate::project::plan::steps(&note)
        .into_iter()
        .enumerate()
        .filter(|(_, s)| s.open())
        .map(|(i, s)| (i + 1, s.text))
        .collect();
    // what the index knows of the notes: drafts, archived ones
    let (drafts, archived) = match crate::index::Index::open(project) {
        Ok(index) => {
            let drafts = index
                .notes_of(crate::note::Kind::Error)
                .unwrap_or_default()
                .into_iter()
                .filter(|n| n.props.get("status").and_then(|s| s.as_str()) == Some("draft"))
                .map(|n| {
                    let seen = n.props.get("last_seen").and_then(|s| s.as_str()).and_then(crate::output::parse_date);
                    (n.id, seen.unwrap_or(n.changed))
                })
                .collect();
            (drafts, index.archived().unwrap_or_default())
        }
        Err(_) => (Vec::new(), Vec::new()),
    };
    let facts = Facts {
        now,
        me: conversation::key().to_string(),
        journal: state.journal(now - LOOK_BACK)?,
        runs: state.runs(None, RUNS)?,
        folders: state.folders(now - LOOK_BACK)?,
        compacted,
        said: state.said()?,
        outdated: found.outdated,
        tidy: found.tidy,
        said_log: state.said_log(now - LOOK_BACK * 4)?,
        steps,
        drafts,
        archived,
        reads: state.reads(now - LOOK_BACK)?,
    };
    crate::output::mark("perceive: done");
    Ok(facts)
}
