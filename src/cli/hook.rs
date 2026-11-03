//! pg hook: what an agent's hooks call (`pg setup` puts them in place, for
//! Claude Code and Codex). The event comes as JSON on stdin; what the agent
//! should know goes back as JSON on stdout. A hook never stands in the
//! agent's way: whatever goes wrong, it ends quietly (and says why in
//! `<home>/hook.log`; with $PARAGMA_HOOK_DEBUG set, every input is
//! written there too).
//!
//! ```text
//! SessionStart            the brief of the project linked to the folder
//! PostToolUse, a command  a failing test or build: the fix memory has for it
//! PostToolUse, an edit    remembers the files the conversation changed
//! Stop                    work done but no handoff: asks once for pg finish
//! ```

use super::Context;
use crate::index::Index;
use crate::output::now;
use crate::state::{State, conversation};
use crate::testrun::{self, Run, parse};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::io::Read;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct Args {
    /// The agent calling: claude or codex
    agent: String,
}

/// The agents whose hooks pg answers.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    pub fn named(name: &str) -> Option<Agent> {
        match name {
            "claude" => Some(Agent::Claude),
            "codex" => Some(Agent::Codex),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }
}

/// Error lines a test run is looked up for.
const EXPLAINED: usize = 3;
/// A known fix shown to a conversation is not shown again for this long.
const SHOWN_AGAIN: i64 = 30 * 60;
/// Most bytes of `hook.log` kept.
const LOG_SIZE: u64 = 200_000;

pub fn run(a: Args) -> Result<()> {
    let Some(agent) = Agent::named(&a.agent) else { bail!("pg hook claude|codex, not {:?}", a.agent) };
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    if std::env::var_os("PARAGMA_HOOK_DEBUG").is_some() {
        log(&format!("{} input: {input}", agent.id()));
    }
    match answer(agent, &input) {
        Ok(Some(out)) => print!("{out}"),
        Ok(None) => {}
        Err(e) => log(&format!("{} {}: {e:#}", agent.id(), event_name(&input))),
    }
    Ok(())
}

/// What to tell the agent about this event, as the agent's JSON.
pub fn answer(agent: Agent, input: &str) -> Result<Option<String>> {
    let event: Value = serde_json::from_str(input)?;
    let name = event["hook_event_name"].as_str().unwrap_or("");
    let session = event["session_id"].as_str().unwrap_or("");
    if !session.is_empty() {
        // the same key the agent's own pg calls have (state/conversation.rs)
        // SAFETY: set before any thread starts; nothing has read it yet
        unsafe { std::env::set_var("PARAGMA_SESSION", conversation::of_agent(agent.id(), session)) };
    }
    let Some(cwd) = event["cwd"].as_str().filter(|c| !c.is_empty()) else { return Ok(None) };
    if std::env::set_current_dir(cwd).is_err() {
        return Ok(None);
    }
    let cx = super::context(None, 1200)?;
    let Some(project) = &cx.project else { return Ok(None) };
    let text = match name {
        "SessionStart" => Some(session_start(&cx, project, event["source"].as_str().unwrap_or(""))?),
        "PostToolUse" | "PostToolUseFailure" => after_tool(&cx, project, &event)?,
        "Stop" if event["stop_hook_active"].as_bool() != Some(true) => stop(project)?,
        _ => None,
    };
    Ok(text.map(|t| reply(agent, name, &t)))
}

/// The JSON that puts `text` in front of the agent.
fn reply(agent: Agent, event: &str, text: &str) -> String {
    let v = match (event, agent) {
        // Codex goes on after Stop only for a block with a reason
        ("Stop", Agent::Codex) => json!({ "decision": "block", "reason": text }),
        _ => json!({ "hookSpecificOutput": { "hookEventName": event, "additionalContext": text } }),
    };
    v.to_string()
}

fn session_start(cx: &Context, project: &crate::project::Project, source: &str) -> Result<String> {
    let brief = super::open::brief_at_start(cx, project)?;
    let why = if source == "compact" { "The conversation was compacted. " } else { "" };
    Ok(format!(
        "{why}Paragma: this folder belongs to project {}. Its brief, as `pg open` gives it (no need to run it again):\n{brief}",
        project.name
    ))
}

/// After a tool: an edit is remembered, a failing test run is explained.
fn after_tool(cx: &Context, project: &crate::project::Project, event: &Value) -> Result<Option<String>> {
    let tool = event["tool_name"].as_str().unwrap_or("");
    let edited = edited_files(tool, &event["tool_input"]);
    if !edited.is_empty() {
        let state = State::open(project)?;
        for f in &edited {
            state.event("edit", f, "")?;
        }
        return Ok(None);
    }
    if tool != "Bash" {
        return Ok(None);
    }
    let command = event["tool_input"]["command"].as_str().unwrap_or("").trim();
    if command.is_empty() || calls_pg(command) {
        return Ok(None); // pg run explains its own runs
    }
    let (output, exit) = tool_output(event);
    let parsed = parse::parse(&output);
    // Codex gives no exit code: what the output says decides
    let exit = exit.unwrap_or(if !parsed.failed.is_empty() || (parsed.passed == 0 && !parsed.errors.is_empty()) {
        1
    } else {
        0
    });
    if parsed.failed.is_empty() && parsed.passed == 0 && (exit == 0 || parsed.errors.is_empty()) {
        return Ok(None); // not a test run or a build that failed
    }
    let code = crate::code::notes::open(&cx.home, project)?;
    let run = Run {
        at: now(),
        command: command.to_string(),
        exit,
        seconds: event["duration_ms"].as_f64().unwrap_or(0.0) / 1000.0,
        failing: parsed.failed.clone(),
        passed: parsed.passed,
        errors: parsed.errors.clone(),
        changed: code.as_ref().map(super::run::changed_files).unwrap_or_default(),
    };
    let state = State::open(project)?;
    let history = state.runs(Some(&run.command), 10)?;
    let verdict = testrun::judge(&run, &history);
    state.record_run(&run)?;
    state.event("run", &run.command, if exit == 0 { "pass" } else { "fail" })?;
    if exit == 0 {
        return Ok(None);
    }

    let mut lines = Vec::new();
    if !verdict.flaky.is_empty() {
        lines.push(format!("flaky (fails now and then, also without a change): {}", verdict.flaky.join(", ")));
    }
    let index = Index::open(project)?;
    let since = now() - SHOWN_AGAIN;
    for line in run.errors.iter().take(EXPLAINED) {
        let known = match super::run::known(&index, project, line)? {
            Some(k) => Some(k),
            None => super::run::known_elsewhere(cx, project, line)?,
        };
        let Some(k) = known else { continue };
        let id = k.split([' ', ':']).next().unwrap_or("").to_string();
        if state.shown_to_me(&id, since)? {
            continue;
        }
        state.shown(std::slice::from_ref(&id), false)?;
        lines.push(format!("{line}\n  known: {k}"));
    }
    if lines.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!("Paragma, about this run:\n{}", lines.join("\n"))))
}

/// Asks once for a handoff when this conversation changed something since
/// its last `pg finish`.
fn stop(project: &crate::project::Project) -> Result<Option<String>> {
    let state = State::open(project)?;
    let events = state.my_events(0)?;
    if events.iter().any(|e| e.action == "ask-finish") {
        return Ok(None);
    }
    if !events.iter().any(|e| matches!(e.action.as_str(), "edit" | "write" | "step" | "run" | "rm" | "mv")) {
        return Ok(None);
    }
    state.event("ask-finish", &project.name, "")?;
    Ok(Some(
        "Paragma: this conversation changed the project and has not handed off. If its work is done or stops \
         here, run: pg finish --done <numbers of the steps it finished> \"what changed, what is next\". If you \
         are only waiting for the user, go on without it."
            .to_string(),
    ))
}

/// The files an edit tool changed: Claude Code's Edit, Write, MultiEdit and
/// NotebookEdit name one; Codex's apply_patch names them in the patch.
fn edited_files(tool: &str, input: &Value) -> Vec<String> {
    match tool {
        "Edit" | "Write" | "MultiEdit" => input["file_path"].as_str().map(|p| vec![p.to_string()]).unwrap_or_default(),
        "NotebookEdit" => input["notebook_path"].as_str().map(|p| vec![p.to_string()]).unwrap_or_default(),
        "apply_patch" => {
            let patch = input["command"].as_str().or_else(|| input["input"].as_str()).unwrap_or("");
            patch
                .lines()
                .filter_map(|l| {
                    ["*** Update File: ", "*** Add File: ", "*** Delete File: ", "*** Move to: "]
                        .iter()
                        .find_map(|p| l.strip_prefix(p))
                })
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        }
        _ => Vec::new(),
    }
}

/// The command's output and exit code (None: not said), as each agent
/// reports them: Claude Code gives stdout and stderr, and a failure as
/// "Exit code N" in `error`; Codex 0.162 gives the output alone, and other
/// versions what the model sees, which may be a list of texts with the
/// result as JSON inside one of them.
fn tool_output(event: &Value) -> (String, Option<i32>) {
    let mut text = String::new();
    let mut exit = None;
    collect(&event["tool_response"], &mut text, &mut exit, 0);
    if let Some(e) = event["error"].as_str() {
        text.push_str(e);
    }
    let exit = exit.or_else(|| exit_in(&text)).or(match event["hook_event_name"].as_str() {
        Some("PostToolUseFailure") => Some(1),
        // Claude Code sends a failed command to PostToolUseFailure
        _ if event["tool_response"].is_object() => Some(0),
        _ => None,
    });
    (text, exit)
}

/// The texts in a tool's response, and the exit code where it says one.
fn collect(v: &Value, text: &mut String, exit: &mut Option<i32>, depth: usize) {
    if depth > 8 {
        return;
    }
    match v {
        Value::String(s) => {
            let t = s.trim_start();
            match serde_json::from_str::<Value>(s) {
                Ok(inner @ (Value::Object(_) | Value::Array(_))) if t.starts_with(['{', '[']) => {
                    collect(&inner, text, exit, depth + 1)
                }
                _ => {
                    text.push_str(s);
                    text.push('\n');
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect(x, text, exit, depth + 1)),
        Value::Object(o) => {
            for (k, x) in o {
                match (k.as_str(), x) {
                    ("exit_code" | "exitCode", Value::Number(n)) => *exit = n.as_i64().map(|n| n as i32).or(*exit),
                    // labels, not output
                    ("type" | "chunk_id" | "id" | "status", _) => {}
                    _ => collect(x, text, exit, depth + 1),
                }
            }
        }
        _ => {}
    }
}

/// The exit code written in a text: "Exit code 1", "Exit code: 1",
/// "Process exited with code 1", or `"exit_code":1` in JSON kept as text.
fn exit_in(text: &str) -> Option<i32> {
    let number = |rest: &str| -> Option<i32> {
        let rest = rest.trim_start_matches([':', ' ']);
        let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '-')).unwrap_or(rest.len());
        rest[..end].parse().ok()
    };
    let lines = text.lines().find_map(|l| {
        let l = l.trim();
        l.strip_prefix("Exit code").or_else(|| l.strip_prefix("Process exited with code")).and_then(number)
    });
    lines.or_else(|| text.find("\"exit_code\":").and_then(|i| number(&text[i + "\"exit_code\":".len()..])))
}

/// Whether a command line runs pg itself.
fn calls_pg(command: &str) -> bool {
    command
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | '"' | '\''))
        .filter(|w| !w.is_empty())
        .any(|w| {
            let base = w.rsplit(['/', '\\']).next().unwrap_or(w);
            base == "pg" || base == "pg.exe"
        })
}

fn event_name(input: &str) -> String {
    serde_json::from_str::<Value>(input)
        .ok()
        .and_then(|v| v["hook_event_name"].as_str().map(str::to_string))
        .unwrap_or_else(|| "?".into())
}

/// Appends to `<home>/hook.log`, starting it again when it grew too big.
fn log(line: &str) {
    let path: PathBuf = crate::project::Home::locate().root.join("hook.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_SIZE) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(f, "{} {line}", now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_name_their_files() {
        let claude = json!({ "file_path": "C:/dev/shop/payments/refund.go", "old_string": "a", "new_string": "b" });
        assert_eq!(edited_files("Edit", &claude), ["C:/dev/shop/payments/refund.go"]);
        let patch = json!({ "command": "*** Begin Patch\n*** Update File: payments/refund.go\n@@\n-a\n+b\n*** Add File: payments/new.go\n+x\n*** End Patch\n" });
        assert_eq!(edited_files("apply_patch", &patch), ["payments/refund.go", "payments/new.go"]);
        assert!(edited_files("Bash", &json!({ "command": "ls" })).is_empty());
    }

    #[test]
    fn outputs_and_exit_codes_of_both_agents() {
        let claude_failed = json!({
            "hook_event_name": "PostToolUseFailure",
            "error": "Exit code 1\n--- FAIL: TestRefund (0.00s)\n    refund_test.go:7: day 30\nFAIL\n"
        });
        let (text, exit) = tool_output(&claude_failed);
        assert_eq!(exit, Some(1));
        assert!(text.contains("refund_test.go:7: day 30"));
        let claude_ok = json!({ "hook_event_name": "PostToolUse", "tool_response": { "stdout": "ok", "stderr": "" } });
        assert_eq!(tool_output(&claude_ok).1, Some(0));
        // Codex 0.162, as it came: the output alone, no exit code
        let codex = json!({ "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": { "command": ".\\fail.cmd" }, "tool_response": "--- FAIL: TestRefund (0.00s)\n    refund_test.go:7: day 30\nFAIL\n" });
        assert_eq!(
            tool_output(&codex),
            ("--- FAIL: TestRefund (0.00s)\n    refund_test.go:7: day 30\nFAIL\n\n".to_string(), None)
        );
        let codex = json!({ "hook_event_name": "PostToolUse", "tool_response": "Exit code: 2\nWall time: 1.2 seconds\nOutput:\nboom" });
        assert_eq!(tool_output(&codex).1, Some(2));
        // what the model sees: texts, the result as JSON inside the second
        let inner = r#"{"chunk_id":"aeed33","wall_time_seconds":0.56,"exit_code":1,"original_token_count":16,"output":"--- FAIL: TestRefund (0.00s)\n    refund_test.go:7: day 30\nFAIL\n"}"#;
        let codex = json!({ "hook_event_name": "PostToolUse", "tool_response": [
            { "type": "input_text", "text": "Script completed\nWall time 1.8 seconds\nOutput:\n" },
            { "type": "input_text", "text": inner }
        ] });
        let (text, exit) = tool_output(&codex);
        assert_eq!(exit, Some(1));
        assert!(text.contains("refund_test.go:7: day 30"), "{text}");
        let passed = json!({ "hook_event_name": "PostToolUse", "tool_response": [{ "type": "input_text", "text": "{\"exit_code\":0,\"output\":\"ok\"}" }] });
        assert_eq!(tool_output(&passed).1, Some(0));
        // the same, flattened into one text
        let flat = json!({ "hook_event_name": "PostToolUse", "tool_response": format!("Script completed\nWall time 1.8 seconds\nOutput:\n{inner}") });
        assert_eq!(tool_output(&flat).1, Some(1));
        let unified = json!({ "hook_event_name": "PostToolUse", "tool_response": "Chunk ID: 1\nWall time: 0.2 seconds\nProcess exited with code 3\nOutput:\nx" });
        assert_eq!(tool_output(&unified).1, Some(3));
    }

    #[test]
    fn commands_that_run_pg() {
        assert!(calls_pg("pg run -- go test ./..."));
        assert!(calls_pg("cd x && pg find refund"));
        assert!(calls_pg("C:/tools/pg.exe open"));
        assert!(!calls_pg("go test ./pkg/..."));
        assert!(!calls_pg("npm run pgsql-migrate"));
    }

    #[test]
    fn replies_in_each_agents_form() {
        let c: Value = serde_json::from_str(&reply(Agent::Claude, "Stop", "x")).unwrap();
        assert_eq!(c["hookSpecificOutput"]["additionalContext"], "x");
        let x: Value = serde_json::from_str(&reply(Agent::Codex, "Stop", "x")).unwrap();
        assert_eq!(x["decision"], "block");
        assert_eq!(x["reason"], "x");
        let s: Value = serde_json::from_str(&reply(Agent::Codex, "SessionStart", "x")).unwrap();
        assert_eq!(s["hookSpecificOutput"]["hookEventName"], "SessionStart");
    }
}
