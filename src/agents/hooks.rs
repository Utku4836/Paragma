//! Paragma's hooks in Claude Code's and Codex's settings: put in place,
//! brought up to date, taken out. Every other hook and setting in the file
//! stays as it is, in its order.
//!
//! Claude Code: `~/.claude/settings.json` (every conversation) or
//! `<folder>/.claude/settings.local.json` (that folder, not shared through
//! git). Codex: `~/.codex/hooks.json` or `<folder>/.codex/hooks.json`.

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// The agents with hooks Paragma can use.
pub const AGENTS: &[&str] = &["claude", "codex"];

/// The events Paragma answers, with the matcher each needs.
fn events(agent: &str) -> &'static [(&'static str, Option<&'static str>)] {
    match agent {
        "claude" => &[
            ("SessionStart", Some("startup|resume|clear|compact")),
            ("PreToolUse", Some("Edit|Write|MultiEdit")),
            ("PostToolUse", Some("Bash|Edit|Write|MultiEdit|NotebookEdit")),
            ("PostToolUseFailure", Some("Bash")),
            ("UserPromptSubmit", None),
            ("Stop", None),
        ],
        _ => &[
            ("SessionStart", Some("startup|resume|clear|compact")),
            ("PreToolUse", Some("apply_patch")),
            ("PostToolUse", Some("Bash|apply_patch")),
            ("UserPromptSubmit", None),
            ("Stop", None),
        ],
    }
}

/// Seconds an agent waits for a hook.
const TIMEOUT: u64 = 30;

/// Where the agent reads hooks: every conversation (`folder` None) or one
/// folder's.
pub fn file(agent: &str, folder: Option<&Path>) -> PathBuf {
    let home = std::env::home_dir().unwrap_or_default();
    match (agent, folder) {
        ("claude", None) => home.join(".claude").join("settings.json"),
        ("claude", Some(f)) => f.join(".claude").join("settings.local.json"),
        (_, None) => codex_home(&home).join("hooks.json"),
        (_, Some(f)) => f.join(".codex").join("hooks.json"),
    }
}

fn codex_home(home: &Path) -> PathBuf {
    match std::env::var_os("CODEX_HOME") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => home.join(".codex"),
    }
}

/// What a change did to the file.
#[derive(Debug, PartialEq)]
pub enum Change {
    Written,
    Unchanged,
}

/// Puts Paragma's hooks into the file (made when missing), replacing the
/// ones an earlier pg put there. `exe`: the pg the hooks run.
pub fn install(agent: &str, path: &Path, exe: &Path) -> Result<Change> {
    let before = read(path)?;
    let mut doc = before.clone();
    let hooks = hooks_object(&mut doc, path)?;
    take_out(hooks);
    for (event, matcher) in events(agent) {
        let mut group = Map::new();
        if let Some(m) = matcher {
            group.insert("matcher".into(), json!(m));
        }
        group.insert("hooks".into(), json!([handler(agent, exe)]));
        let list = hooks.entry(*event).or_insert_with(|| json!([]));
        match list.as_array_mut() {
            Some(a) => a.push(Value::Object(group)),
            None => bail!("{}: hooks.{event} is not a list", path.display()),
        }
    }
    write_if_changed(path, &before, &doc)
}

/// Takes Paragma's hooks out of the file; other hooks stay.
pub fn remove(path: &Path) -> Result<Change> {
    if !path.is_file() {
        return Ok(Change::Unchanged);
    }
    let before = read(path)?;
    let mut doc = before.clone();
    let hooks = hooks_object(&mut doc, path)?;
    take_out(hooks);
    if hooks.is_empty() {
        doc.as_object_mut().map(|o| o.remove("hooks"));
    }
    write_if_changed(path, &before, &doc)
}

/// Whether the file holds Paragma's hooks.
pub fn installed(path: &Path) -> bool {
    let Ok(doc) = read(path) else { return false };
    doc["hooks"].as_object().is_some_and(|h| {
        h.values()
            .filter_map(Value::as_array)
            .flatten()
            .any(|g| g["hooks"].as_array().is_some_and(|l| l.iter().any(ours)))
    })
}

/// How the agent runs pg. Claude Code runs a program with its arguments as
/// given (no shell on the way); Codex runs a command line, in whichever
/// shell, so the path goes without quotes and with forward slashes, which
/// cmd, PowerShell and sh all read the same.
fn handler(agent: &str, exe: &Path) -> Value {
    let exe_s = exe.to_string_lossy().replace('\\', "/");
    if agent == "claude" {
        return json!({ "type": "command", "command": exe_s, "args": ["hook", "claude"], "timeout": TIMEOUT });
    }
    // a path with spaces would need quotes, which shells read differently
    let program = if exe_s.contains(' ') { "pg".to_string() } else { exe_s };
    json!({ "type": "command", "command": format!("{program} hook codex"), "timeout": TIMEOUT, "statusMessage": "Paragma" })
}

/// A hook pg put in place: it runs `pg hook <agent>`.
fn ours(h: &Value) -> bool {
    let mut line = h["command"].as_str().unwrap_or("").to_string();
    for a in h["args"].as_array().into_iter().flatten() {
        line.push(' ');
        line.push_str(a.as_str().unwrap_or(""));
    }
    let words: Vec<&str> = line.split_whitespace().collect();
    words.windows(3).any(|w| {
        let base = w[0].rsplit(['/', '\\']).next().unwrap_or(w[0]).to_lowercase();
        let base = base.strip_suffix(".exe").unwrap_or(&base);
        // pg, or the build npm downloads: pg-<system>-<arch>
        let pg = base == "pg" || ["pg-win32-", "pg-linux-", "pg-darwin-"].iter().any(|p| base.starts_with(p));
        pg && w[1] == "hook" && AGENTS.contains(&w[2])
    })
}

/// Drops Paragma's handlers, then the groups and events left empty by that.
fn take_out(hooks: &mut Map<String, Value>) {
    for list in hooks.values_mut() {
        let Some(groups) = list.as_array_mut() else { continue };
        for g in groups.iter_mut() {
            if let Some(handlers) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                handlers.retain(|h| !ours(h));
            }
        }
        groups.retain(|g| g["hooks"].as_array().is_none_or(|h| !h.is_empty()));
    }
    hooks.retain(|_, list| list.as_array().is_none_or(|a| !a.is_empty()));
}

fn read(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        Ok(text) => {
            let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
                .with_context(|| format!("{} is not valid JSON; it was left as it is", path.display()))?;
            if !v.is_object() {
                bail!("{} is not a JSON object; it was left as it is", path.display());
            }
            Ok(v)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

fn hooks_object<'a>(doc: &'a mut Value, path: &Path) -> Result<&'a mut Map<String, Value>> {
    let root = doc.as_object_mut().expect("read gives an object");
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    match hooks.as_object_mut() {
        Some(h) => Ok(h),
        None => bail!("{}: \"hooks\" is not an object; it was left as it is", path.display()),
    }
}

fn write_if_changed(path: &Path, before: &Value, after: &Value) -> Result<Change> {
    if before == after {
        return Ok(Change::Unchanged);
    }
    let text = serde_json::to_string_pretty(after)? + "\n";
    crate::project::files::write_atomic(path, text.as_bytes())?;
    Ok(Change::Written)
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/agents/hooks.rs"]
mod tests;
