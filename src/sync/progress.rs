//! How far a sync, a move or a check is: what it does and how many files
//! of how many, in <home>/git/progress.json while it runs, for the
//! interface's work band. Written at most a few times a second.

use serde_json::json;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static LAST: Mutex<Option<Instant>> = Mutex::new(None);
const EVERY: Duration = Duration::from_millis(150);

fn path(home: &crate::project::Home) -> std::path::PathBuf {
    super::dir(home).join("progress.json")
}

/// `what` ("sending", "checking"): `done` of `all` files.
pub fn set(home: &crate::project::Home, what: &str, done: usize, all: usize) {
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if done != 0 && done != all && last.is_some_and(|t| t.elapsed() < EVERY) {
        return;
    }
    *last = Some(Instant::now());
    let v = json!({ "what": what, "done": done, "all": all, "at": crate::output::now() });
    let _ = std::fs::write(path(home), v.to_string());
}

pub fn clear(home: &crate::project::Home) {
    let _ = std::fs::remove_file(path(home));
}

/// What is going on now, for people ("sending 312 of 1230 files"); None
/// when nothing is (or it is stale).
pub fn read(home: &crate::project::Home) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path(home)).ok()?).ok()?;
    if crate::output::now() - v["at"].as_i64()? > 30 {
        return None;
    }
    let (done, all) = (v["done"].as_u64()?, v["all"].as_u64()?);
    let what = v["what"].as_str()?;
    if all == 0 {
        return Some(what.to_string());
    }
    Some(format!("{what} {done} of {}", crate::output::count(all as i64, "file")))
}

/// Takes the progress out when it is dropped (the work ended, well or not).
pub struct Done<'a>(pub &'a crate::project::Home);

impl Drop for Done<'_> {
    fn drop(&mut self) {
        clear(self.0);
    }
}
