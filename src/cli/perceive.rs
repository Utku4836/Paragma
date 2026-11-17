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

pub fn facts(_project: &Project, state: &State, compacted: bool) -> Result<Facts> {
    let now = now();
    Ok(Facts {
        now,
        me: conversation::key().to_string(),
        journal: state.journal(now - LOOK_BACK)?,
        runs: state.runs(None, RUNS)?,
        folders: state.folders(now - LOOK_BACK)?,
        compacted,
        said: state.said()?,
        outdated: super::sleep::found(state).outdated,
        tidy: super::sleep::found(state).tidy,
        said_log: state.said_log(now - LOOK_BACK * 4)?,
    })
}
