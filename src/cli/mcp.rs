//! pg mcp: Paragma as an MCP server, over stdio (JSON-RPC, one message a
//! line). A thin layer: its tools run pg's own commands, so they say exactly
//! what pg says; its instructions, which a client puts in the agent's
//! context when the conversation starts, carry the brief of the project
//! linked to the folder, so clients without hooks get it too.
//!
//! ```text
//! claude mcp add paragma -- pg mcp        (Claude Code; other clients alike)
//! ```

use super::Context;
use anyhow::Result;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

#[derive(clap::Args)]
pub struct Args {}

/// The protocol versions this server speaks, newest first.
const VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

pub fn run(cx: &Context, _a: Args) -> Result<()> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => answer(cx, &msg),
            Err(e) => Some(error(Value::Null, -32700, &format!("not JSON: {e}"))),
        };
        if let Some(r) = reply {
            writeln!(out, "{r}")?;
            out.flush()?;
        }
    }
    Ok(())
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// The reply to one message; None for a notification.
fn answer(cx: &Context, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned()?; // a notification (no id) gets no reply
    let method = msg["method"].as_str().unwrap_or("");
    Some(match method {
        "initialize" => {
            let asked = msg["params"]["protocolVersion"].as_str().unwrap_or(VERSIONS[0]);
            let version = if VERSIONS.contains(&asked) { asked } else { VERSIONS[0] };
            ok(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "paragma", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": instructions(cx),
                }),
            )
        }
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({ "tools": tools() })),
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let args = &msg["params"]["arguments"];
            match command(name, args) {
                Some(argv) => {
                    let (code, text) = pg(cx, &argv);
                    // 1: memory has nothing on it, an answer, not a failure
                    ok(id, json!({ "content": [{ "type": "text", "text": text }], "isError": code > 1 }))
                }
                None => error(id, -32602, &format!("no tool {name:?}, or its arguments are missing")),
            }
        }
        _ => error(id, -32601, &format!("no method {method:?}")),
    })
}

/// How to use Paragma, and the brief of the project linked to the folder.
fn instructions(cx: &Context) -> String {
    let core = crate::skills::CORE;
    let tools = "Here the pg commands are tools: pg_open, pg_find, pg_read, pg_write, pg_run, pg_steps, pg_finish, \
                 pg_tell, and pg for any other command (its arguments as a list).";
    let brief = match &cx.project {
        Some(_) => {
            let (code, text) = pg(cx, &["open".to_string()]);
            if code == 0 { format!("\n\nThe brief of this folder's project (pg open):\n{text}") } else { String::new() }
        }
        None => "\n\nNo project is linked to this folder: pg_open with a name starts or continues one.".to_string(),
    };
    format!("{core}\n{tools}{brief}")
}

/// Runs pg itself with these arguments, in the folder the server was
/// started in: its exit code and what it printed.
fn pg(cx: &Context, argv: &[String]) -> (i32, String) {
    let Ok(exe) = std::env::current_exe() else { return (7, "pg not found".into()) };
    match std::process::Command::new(exe).args(argv).current_dir(&cx.cwd).stdin(std::process::Stdio::null()).output() {
        Ok(o) => (
            o.status.code().unwrap_or(7),
            format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
                .trim_end()
                .to_string(),
        ),
        Err(e) => (7, format!("pg could not start: {e}")),
    }
}

fn s(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().map(str::to_string).filter(|x| !x.is_empty())
}

fn list(v: &Value, key: &str) -> Vec<String> {
    match &v[key] {
        Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        Value::String(x) if !x.is_empty() => vec![x.clone()],
        _ => Vec::new(),
    }
}

/// A tool call as pg's arguments; None when it cannot be one.
pub fn command(name: &str, a: &Value) -> Option<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    let flag = |v: &mut Vec<String>, f: &str, x: Option<String>| {
        if let Some(x) = x {
            v.push(f.to_string());
            v.push(x);
        }
    };
    match name {
        "pg_open" => {
            v.push("open".into());
            v.extend(s(a, "name"));
            flag(&mut v, "--goal", s(a, "goal"));
            for st in list(a, "steps") {
                flag(&mut v, "--step", Some(st));
            }
            for d in list(a, "decisions") {
                flag(&mut v, "--decision", Some(d));
            }
        }
        "pg_find" => {
            v.push("find".into());
            v.push(s(a, "query")?);
            if a["all"].as_bool() == Some(true) {
                v.push("--all".into());
            }
        }
        "pg_read" => {
            v.push("read".into());
            v.push(s(a, "target")?);
        }
        "pg_write" => {
            v.push("write".into());
            if let Some(r) = s(a, "rule") {
                flag(&mut v, "--rule", Some(r));
            } else if let Some(d) = s(a, "decision") {
                flag(&mut v, "--decision", Some(d));
                flag(&mut v, "--why", s(a, "why"));
            } else if let Some(e) = s(a, "error") {
                flag(&mut v, "--error", Some(e));
                flag(&mut v, "--cause", s(a, "cause"));
                flag(&mut v, "--fix", s(a, "fix"));
            } else {
                v.push(s(a, "path")?);
                v.push(s(a, "text")?);
                if a["append"].as_bool() == Some(true) {
                    v.push("--append".into());
                }
            }
        }
        "pg_run" => {
            v.push("run".into());
            v.push("--".into());
            v.push(s(a, "command")?);
        }
        "pg_steps" => {
            v.push("steps".into());
            if let Some(n) = a["take"].as_u64() {
                flag(&mut v, "--take", Some(n.to_string()));
            }
            if a["all"].as_bool() == Some(true) {
                v.push("--all".into());
            }
        }
        "pg_finish" => {
            v.push("finish".into());
            v.push(s(a, "summary")?);
            flag(&mut v, "--done", s(a, "done"));
        }
        "pg_tell" => {
            v.push("tell".into());
            v.push(s(a, "text")?);
        }
        "pg" => {
            v = list(a, "args");
            // the server is not started again from inside itself
            if v.first().is_none_or(|c| c == "mcp") {
                return None;
            }
        }
        _ => return None,
    }
    Some(v)
}

/// The tools, each with what its arguments are.
fn tools() -> Value {
    let t = |name: &str, about: &str, props: Value, required: &[&str]| json!({ "name": name, "description": about, "inputSchema": { "type": "object", "properties": props, "required": required } });
    let text = |about: &str| json!({ "type": "string", "description": about });
    json!([
        t(
            "pg_open",
            "Start or continue a project's work and get its brief (goal, steps, last handoff, rules, likely code). Alone: the project of this folder.",
            json!({
                "name": text("The project (omit: the one linked to this folder)"),
                "goal": text("The goal of the work"),
                "steps": { "type": "array", "items": { "type": "string" }, "description": "Every step of the user's plan" },
                "decisions": { "type": "array", "items": { "type": "string" }, "description": "Decisions the user made" }
            }),
            &[],
        ),
        t(
            "pg_find",
            "Search memory before searching files: a question, an error line, a code name, or '<library> <names>' for a library's real API.",
            json!({ "query": text("What to find"), "all": { "type": "boolean", "description": "Every project" } }),
            &["query"],
        ),
        t(
            "pg_read",
            "Read a record (e12, r3, d7), a note's path, or code: file#Function, file:10-20.",
            json!({ "target": text("What to read") }),
            &["target"],
        ),
        t(
            "pg_write",
            "Keep what the next conversation needs: a rule, a decision (with why), an error (with cause and fix), or a note (path and text).",
            json!({
                "rule": text("A rule, in a sentence"),
                "decision": text("A decision"), "why": text("Why it was decided"),
                "error": text("The error line as printed"), "cause": text("What caused it"), "fix": text("How it was fixed"),
                "path": text("A note's path: folder/name"), "text": text("The note's text"),
                "append": { "type": "boolean", "description": "Add to the note instead of replacing it" }
            }),
            &[],
        ),
        t(
            "pg_run",
            "Run tests, a build or a command that needs a key ($NAME): what fails, what is new, the values compared, the fix memory has.",
            json!({ "command": text("The command line") }),
            &["command"],
        ),
        t(
            "pg_steps",
            "The plan's steps with their numbers; take one so other conversations leave it.",
            json!({ "take": { "type": "integer", "description": "Take this step" }, "all": { "type": "boolean", "description": "Done and deferred ones too" } }),
            &[],
        ),
        t(
            "pg_finish",
            "Before the final answer on real work: what was done and what is next, and the steps it finished.",
            json!({ "summary": text("What was done; what is next"), "done": text("Numbers of the steps finished: 1,3") }),
            &["summary"],
        ),
        t(
            "pg_tell",
            "A short note for the other conversations on this project (each sees it once).",
            json!({ "text": text("What they should know") }),
            &["text"],
        ),
        t(
            "pg",
            "Any other pg command, its arguments as a list: [\"ripple\", \"Refund\"], [\"doctor\"], [\"skill\", \"coding\"].",
            json!({ "args": { "type": "array", "items": { "type": "string" } } }),
            &["args"],
        ),
    ])
}

#[cfg(all(test, paragma_local))]
#[path = "../../unit/src/cli/mcp.rs"]
mod tests;
