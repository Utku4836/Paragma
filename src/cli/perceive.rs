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

/// Notes from the other conversations not said to this one yet, and what
/// this conversation's own work calls for (a plan, a handoff), as lines;
/// remembered as said. Cheap: the log of the last day and the project note.
pub fn news(project: &Project, state: &State) -> Result<Vec<String>> {
    let now = now();
    let f = Facts {
        now,
        me: conversation::key().to_string(),
        journal: state.journal(now - crate::kernel::experts::NEWS)?,
        folders: state.folders(now - LOOK_BACK)?,
        said: state.said()?,
        steps: open_steps(project),
        ..Facts::default()
    };
    let hints = crate::kernel::news_hints(&f, 200);
    state.say(&hints.iter().map(|h| h.key.clone()).collect::<Vec<_>>())?;
    Ok(hints.into_iter().map(|h| h.text).collect())
}

/// The plan's open steps: number and text.
fn open_steps(project: &Project) -> Vec<(usize, String)> {
    let note = crate::project::files::read_text(&project.note_file(crate::project::plan::FILE))
        .map(|t| crate::note::Note::parse(&t))
        .unwrap_or_default();
    crate::project::plan::steps(&note)
        .into_iter()
        .enumerate()
        .filter(|(_, s)| s.open())
        .map(|(i, s)| (i + 1, s.text))
        .collect()
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
    let mut dues: Vec<(String, String, String)> = Vec::new();
    let (drafts, archived) = match crate::index::Index::open(project) {
        Ok(index) => {
            for kind in [crate::note::Kind::Note, crate::note::Kind::Decision, crate::note::Kind::Error] {
                for n in index.notes_of(kind).unwrap_or_default() {
                    if let Some(d) = n.props.get("due").and_then(|v| v.as_str()) {
                        let title = n.props.get("title").and_then(|v| v.as_str()).unwrap_or(&n.path).to_string();
                        dues.push((n.id.clone(), title, d.to_string()));
                    }
                }
            }
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
        dues,
    };
    crate::output::mark("perceive: done");
    Ok(facts)
}
