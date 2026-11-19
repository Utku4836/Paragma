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
pub struct Args {
    /// The app that starts it (pg setup writes it): with Paragma's hooks in
    /// that app, the brief comes from them and is not said again
    #[arg(long)]
    client: Option<String>,
}

/// The protocol versions this server speaks, newest first.
const VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let hooked = a.client.as_deref().is_some_and(|c| crate::agents::mcp::hooks_give_the_brief(c, &cx.cwd));
    let stdin = std::io::stdin();
    let out = std::sync::Arc::new(std::sync::Mutex::new(std::io::stdout()));
    let say = |out: &std::sync::Mutex<std::io::Stdout>, r: &Value| -> std::io::Result<()> {
        let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
        writeln!(o, "{r}")?;
        o.flush()
    };
    // tool calls run side by side: an app that sends several at once gets
    // each answer as soon as it is ready (pg's commands are safe together)
    let mut running = Vec::new();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => msg,
            Err(e) => {
                say(&out, &error(Value::Null, -32700, &format!("not JSON: {e}")))?;
                continue;
            }
        };
        if msg["method"] == "tools/call"
            && let Some(id) = msg.get("id").cloned()
        {
            let (out, cwd) = (out.clone(), cx.cwd.clone());
            running.retain(|t: &std::thread::JoinHandle<()>| !t.is_finished());
            running.push(std::thread::spawn(move || {
                let _ = say(&out, &call(&cwd, id, &msg["params"]));
            }));
            continue;
        }
        if let Some(r) = answer(cx, &msg, hooked) {
            say(&out, &r)?;
        }
    }
    // the input closed: the calls still running answer first
    for t in running {
        let _ = t.join();
    }
    Ok(())
}

/// The answer to one tool call: the pg command it stands for, run in `cwd`.
fn call(cwd: &std::path::Path, id: Value, params: &Value) -> Value {
    let name = params["name"].as_str().unwrap_or("");
    if name == "pg_batch" {
        return match batch_commands(&params["arguments"]) {
            Ok(commands) => {
                let done = super::batch::run_all(cwd, &commands);
                let failed = done.iter().any(|(c, _)| *c > 1);
                let text = super::batch::report(&commands, &done);
                ok(id, json!({ "content": [{ "type": "text", "text": text.trim_end() }], "isError": failed }))
            }
            Err(e) => error(id, -32602, &e),
        };
    }
    match command(name, &params["arguments"]) {
        Some(argv) => {
            let (code, text) = pg(cwd, &argv);
            // 1: memory has nothing on it, an answer, not a failure
            ok(id, json!({ "content": [{ "type": "text", "text": text }], "isError": code > 1 }))
        }
        None => error(id, -32602, &format!("no tool {name:?}, or its arguments are missing")),
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// The reply to one message; None for a notification. `hooked`: the app's
/// hooks give the brief.
fn answer(cx: &Context, msg: &Value, hooked: bool) -> Option<Value> {
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
                    "instructions": instructions(cx, hooked),
                }),
            )
        }
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({ "tools": tools() })),
        "tools/call" => call(&cx.cwd, id, &msg["params"]),
        _ => error(id, -32601, &format!("no method {method:?}")),
    })
}

/// How to use Paragma, and the brief of the project linked to the folder
/// (unless the app's hooks give it: the agent would read it twice).
fn instructions(cx: &Context, hooked: bool) -> String {
    let core = crate::skills::CORE;
    let tools = "Here the pg commands are tools (pg_open, pg_find, pg_read, pg_write, pg_run, pg_steps, pg_finish, \
                 pg_tell, pg_ask, pg_answer, pg_ripple, pg_tree, pg_undo; pg for any other command), each taking \
                 project and budget. Several lookups at once: one pg_batch call, or calls sent together; they run \
                 side by side.";
    let brief = match &cx.project {
        Some(_) if hooked => "\n\nParagma's hooks give this conversation the project's brief at its start.".to_string(),
        Some(_) => {
            let (code, text) = pg(&cx.cwd, &["open".to_string()]);
            if code == 0 { format!("\n\nThe brief of this folder's project (pg open):\n{text}") } else { String::new() }
        }
        None => "\n\nNo project is linked to this folder: pg_open with a name starts or continues one.".to_string(),
    };
    format!("{core}\n{tools}{brief}")
}

/// Runs pg itself with these arguments, in the folder the server was
/// started in: its exit code and what it printed.
fn pg(cwd: &std::path::Path, argv: &[String]) -> (i32, String) {
    super::batch::pg(cwd, argv)
}

fn s(v: &Value, key: &str) -> Option<String> {
    match &v[key] {
        Value::String(x) if !x.is_empty() => Some(x.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn list(v: &Value, key: &str) -> Vec<String> {
    match &v[key] {
        Value::Array(a) => a
            .iter()
            .filter_map(|x| match x {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect(),
        Value::String(x) if !x.is_empty() => vec![x.clone()],
        _ => Vec::new(),
    }
}

/// A tool call as pg's arguments; None when it cannot be one. Every tool
/// takes `project` (another project than the folder's) and `budget` (most
/// tokens of output), as -p and --budget.
pub fn command(name: &str, a: &Value) -> Option<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    let flag = |v: &mut Vec<String>, f: &str, x: Option<String>| {
        if let Some(x) = x {
            v.push(f.to_string());
            v.push(x);
        }
    };
    let numbers = |key: &str| -> Option<String> {
        let l = list(a, key);
        (!l.is_empty()).then(|| l.join(","))
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
            flag(&mut v, "--in", s(a, "in"));
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
            } else if let Some(c) = s(a, "confirm") {
                flag(&mut v, "--confirm", Some(c));
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
            flag(&mut v, "--take", s(a, "take"));
            for (key, f) in [("drop", "--drop"), ("defer", "--defer"), ("resume", "--resume"), ("reopen", "--reopen")] {
                flag(&mut v, f, numbers(key));
            }
            if let Some(n) = s(a, "edit") {
                flag(&mut v, "--edit", Some(n));
                v.push(s(a, "text")?);
            }
            if a["all"].as_bool() == Some(true) {
                v.push("--all".into());
            }
        }
        "pg_finish" => {
            v.push("finish".into());
            v.push(s(a, "summary")?);
            flag(&mut v, "--done", numbers("done"));
            for l in list(a, "left") {
                flag(&mut v, "--left", Some(l));
            }
        }
        "pg_tell" => {
            v.push("tell".into());
            v.push(s(a, "text")?);
        }
        "pg_ask" => {
            v.push("ask".into());
            v.extend(s(a, "question"));
        }
        "pg_answer" => {
            v.push("answer".into());
            match &a["answers"] {
                Value::Null => {
                    v.push(s(a, "question")?);
                    v.push(s(a, "answer")?);
                }
                many => {
                    v.push("--json".into());
                    v.push(many.to_string());
                }
            }
        }
        "pg_ripple" => {
            v.push("ripple".into());
            v.extend(s(a, "target"));
        }
        "pg_tree" => {
            v.push("tree".into());
            v.extend(s(a, "path"));
        }
        "pg_undo" => {
            v.push("undo".into());
            v.extend(s(a, "which"));
        }
        "pg" => {
            v = list(a, "args");
            // the server is not started again from inside itself
            if v.first().is_none_or(|c| matches!(c.as_str(), "mcp" | "hook")) {
                return None;
            }
        }
        _ => return None,
    }
    // -p and --budget first: after `run --` they would be the command's
    let mut out = Vec::new();
    flag(&mut out, "--project", s(a, "project"));
    flag(&mut out, "--budget", s(a, "budget"));
    out.append(&mut v);
    Some(out)
}

/// pg_batch's calls as pg commands: each a tool call ({"tool": "pg_find",
/// "arguments": {...}}) or pg's arguments as a list. The batch's own
/// project and budget go to a call that names none.
pub fn batch_commands(a: &Value) -> Result<Vec<Vec<String>>, String> {
    let Some(calls) = a["calls"].as_array().filter(|c| !c.is_empty()) else {
        return Err("give calls: [{\"tool\": \"pg_find\", \"arguments\": {\"query\": \"...\"}}, ...]".into());
    };
    if calls.len() > super::batch::MOST {
        return Err(format!("{} calls; at most {} in one batch", calls.len(), super::batch::MOST));
    }
    let mut out = Vec::new();
    for (i, c) in calls.iter().enumerate() {
        let (tool, mut args) = match c {
            Value::Array(_) => ("pg".to_string(), json!({ "args": c })),
            _ => (
                c["tool"].as_str().or(c["name"].as_str()).unwrap_or("").to_string(),
                if c["arguments"].is_object() { c["arguments"].clone() } else { json!({}) },
            ),
        };
        if tool == "pg_batch" {
            return Err(format!("call {}: a batch inside a batch", i + 1));
        }
        for key in ["project", "budget"] {
            if args.get(key).is_none() && !a[key].is_null() {
                args[key] = a[key].clone();
            }
        }
        match command(&tool, &args) {
            Some(argv) if !matches!(argv.iter().find(|x| !x.starts_with('-')).map(String::as_str), Some("batch")) => {
                out.push(argv)
            }
            _ => return Err(format!("call {}: no tool {tool:?}, or its arguments are missing", i + 1)),
        }
    }
    Ok(out)
}

/// The tools, each with what its arguments are.
fn tools() -> Value {
    let text = |about: &str| json!({ "type": "string", "description": about });
    let numbers = |about: &str| json!({ "type": "array", "items": { "type": "integer" }, "description": about });
    // every tool may name another project and a budget
    let t = |name: &str, about: &str, mut props: Value, required: &[&str]| {
        props["project"] = json!({ "type": "string", "description": "Another project than this folder's" });
        props["budget"] = json!({ "type": "integer", "description": "Most tokens of output (default 1200)" });
        json!({ "name": name, "description": about, "inputSchema": { "type": "object", "properties": props, "required": required } })
    };
    json!([
        t(
            "pg_open",
            "Start or continue a project's work and get its brief (goal, steps, last handoff, rules, likely code). Alone: the project of this folder. Give every step and decision of the user's plan in this one call.",
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
            json!({ "query": text("What to find"), "all": { "type": "boolean", "description": "Every project" }, "in": text("Search this other project") }),
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
            "Keep what the next conversation needs: a rule, a decision (with why), an error (with cause and fix), a note (path and text); confirm: a record still right, or a draft pg wrote kept.",
            json!({
                "rule": text("A rule, in a sentence"),
                "decision": text("A decision"), "why": text("Why it was decided"),
                "error": text("The error line as printed"), "cause": text("What caused it"), "fix": text("How it was fixed"),
                "path": text("A note's path: folder/name"), "text": text("The note's text"),
                "append": { "type": "boolean", "description": "Add to the note instead of replacing it" },
                "confirm": text("A record's id: still right as the code is now (or a draft kept)")
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
            "The plan's steps with their numbers; take one so other conversations leave it; drop, defer, resume, reopen or edit one.",
            json!({
                "take": { "type": "integer", "description": "Take this step" },
                "drop": numbers("Steps gone stale or wrong"),
                "defer": numbers("Steps left for later on purpose"),
                "resume": numbers("Deferred steps back in the plan"),
                "reopen": numbers("Steps closed by mistake"),
                "edit": { "type": "integer", "description": "Rewrite this step (with text)" },
                "text": text("The step's new text, with edit"),
                "all": { "type": "boolean", "description": "Done and deferred ones too" }
            }),
            &[],
        ),
        t(
            "pg_finish",
            "Before the final answer on real work: what was done and what is next, and the steps it finished.",
            json!({
                "summary": text("What was done; what is next"),
                "done": numbers("Numbers of the steps finished"),
                "left": { "type": "array", "items": { "type": "string" }, "description": "Steps left for later" }
            }),
            &["summary"],
        ),
        t(
            "pg_tell",
            "A short note for the other conversations on this project (each sees it once).",
            json!({ "text": text("What they should know") }),
            &["text"],
        ),
        t(
            "pg_ask",
            "Keep a question that waits on the user (the brief lists it until answered); without one: the open questions.",
            json!({ "question": text("The question for the user") }),
            &[],
        ),
        t(
            "pg_answer",
            "The user's answer to a kept question (by its number or words only it has); answers: many at once.",
            json!({
                "question": text("Its number, or words only it has"),
                "answer": text("The user's answer"),
                "answers": { "type": "array", "items": { "type": "object" }, "description": "Many: [{\"question\": \"2\", \"answer\": \"...\"}]" }
            }),
            &[],
        ),
        t(
            "pg_ripple",
            "Before changing a function's signature, name or existence: its callers and tests; without a target: callers a change left behind.",
            json!({ "target": text("A function, method or type") }),
            &[],
        ),
        t(
            "pg_tree",
            "The map of the project's code: files and their functions, with the notes about them marked.",
            json!({ "path": text("A folder or file (default: all)") }),
            &[],
        ),
        t(
            "pg_undo",
            "What pg did by itself (a handoff, a draft record, steps closed): listed; with a number, taken back.",
            json!({ "which": text("Its number, or last") }),
            &[],
        ),
        t(
            "pg_batch",
            "Several calls at once, each answered in order: those that only look (find, read, ripple, tree) run side by side, one that writes waits for those before it.",
            json!({
                "calls": {
                    "type": "array",
                    "description": "[{\"tool\": \"pg_find\", \"arguments\": {\"query\": \"refund\"}}, {\"tool\": \"pg_read\", \"arguments\": {\"target\": \"e12\"}}]",
                    "items": { "type": "object" }
                }
            }),
            &["calls"],
        ),
        t(
            "pg",
            "Any other pg command, its arguments as a list: [\"doctor\"], [\"skill\", \"coding\"], [\"ls\"].",
            json!({ "args": { "type": "array", "items": { "type": "string" } } }),
            &["args"],
        ),
    ])
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/cli/mcp.rs"]
mod tests;
