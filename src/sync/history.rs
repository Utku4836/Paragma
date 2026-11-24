//! What sync did on this computer, newest last: each sync that sent or
//! brought something or failed, each connect, move, disconnect. The last
//! few hundred are kept, in <home>/git/history.jsonl.

use serde_json::{Value, json};

const KEEP: usize = 300;

/// One thing sync did: when, what, whether it went well.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub at: i64,
    pub what: String,
    pub ok: bool,
}

fn path(home: &crate::project::Home) -> std::path::PathBuf {
    super::dir(home).join("history.jsonl")
}

/// Adds one (never fails what it is said of).
pub fn add(home: &crate::project::Home, what: &str, ok: bool) {
    let mut lines: Vec<String> =
        std::fs::read_to_string(path(home)).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default();
    lines.push(json!({ "at": crate::output::now(), "what": what, "ok": ok }).to_string());
    let from = lines.len().saturating_sub(KEEP);
    let _ = std::fs::create_dir_all(super::dir(home));
    let _ = crate::project::files::write_atomic(&path(home), (lines[from..].join("\n") + "\n").as_bytes());
}

/// The last `n`, newest first.
pub fn last(home: &crate::project::Home, n: usize) -> Vec<Entry> {
    let text = std::fs::read_to_string(path(home)).unwrap_or_default();
    text.lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|v| Entry {
            at: v["at"].as_i64().unwrap_or(0),
            what: v["what"].as_str().unwrap_or("").to_string(),
            ok: v["ok"].as_bool().unwrap_or(true),
        })
        .take(n)
        .collect()
}
