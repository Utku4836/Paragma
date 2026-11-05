//! pg hook: what an agent's hooks call (`pg setup` puts them in place, for
//! Claude Code and Codex). The event comes as JSON on stdin; what the agent
//! should know goes back as JSON on stdout. A hook never stands in the
//! agent's way: whatever goes wrong, it ends quietly (and says why in
//! `<home>/hook.log`; with $PARAGMA_HOOK_DEBUG set, every input is
//! written there too).
//!
//! ```text
//! SessionStart            the brief of the project linked to the folder
//! PreToolUse, an edit     the rules, decisions and notes binding the code it
//!                         touches: the edit waits once until they are read
//! PostToolUse, a command  a failing test or build: the fix memory has for it
//! PostToolUse, an edit    the other notes about that code (past errors of the
//!                         file), and remembers the files changed
//! UserPromptSubmit        a lasting rule in the user's words: asks to keep it;
//!                         decisions or a plan: asks to record them first
//! Stop                    work done but no handoff: asks once for pg finish
//! ```

use super::Context;
use crate::code::notes::{CodeNotes, Linked, about_lines_by};
use crate::index::Index;
use crate::note::{Kind, Note};
use crate::output::{now, one_line};
use crate::project::files;
use crate::state::links::{Links, folder_key};
use crate::state::{State, conversation};
use crate::testrun::{self, Run, parse};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};

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
/// Most notes shown before an edit.
const JUST_IN_TIME: usize = 5;
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
        "PreToolUse" => before_edit(&cx, project, &event)?,
        "UserPromptSubmit" => user_said(project, event["prompt"].as_str().unwrap_or(""))?,
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
        // context given before a tool reaches the model only with the tool's
        // result, after the edit: the edit waits until the memory is read
        ("PreToolUse", _) => json!({ "hookSpecificOutput": {
            "hookEventName": event,
            "permissionDecision": "deny",
            "permissionDecisionReason": text,
        } }),
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

/// Before an edit: the rules and notes tied to the code it touches (the
/// file, the functions and types around the changed lines), each once a
/// conversation. Only what names that code: nothing found by likeness.
///
/// The edit waits (once: everything found counts as shown) only for what
/// binds it: a rule or decision on that code, or a note naming the
/// function it changes. Notes that only name the file (past errors there)
/// come after the edit instead, with no turn lost.
fn before_edit(cx: &Context, project: &crate::project::Project, event: &Value) -> Result<Option<String>> {
    let cwd = cx.cwd.clone();
    let targets = edit_targets(event["tool_name"].as_str().unwrap_or(""), &event["tool_input"], &cwd);
    if targets.is_empty() {
        return Ok(None);
    }
    let folders = Links::load(&cx.home).folders_of(&project.name);
    let index = Index::open(project)?;
    let notes = CodeNotes::load(&index)?;
    let state = State::open(project)?;
    // since this conversation started, or was compacted and lost what it saw
    let since = state.my_last("open").unwrap_or(0);
    let mut found: Vec<(&Linked, bool)> = Vec::new();
    for t in &targets {
        let Some(rel) = relative(&folders, &t.file) else { continue };
        let mut about = about_lines_by(&notes, &rel.1, &t.source, t.lines.as_deref());
        if folders.len() > 1 {
            // notes in a project of many folders may name the folder too
            let name = rel.0.rsplit('/').next().unwrap_or("");
            about.extend(about_lines_by(&notes, &format!("{name}/{}", rel.1), &t.source, t.lines.as_deref()));
        }
        for (l, by_symbol) in about {
            if found.iter().any(|f| f.0.id == l.id) || state.shown_to_me(&l.id, since)? {
                continue;
            }
            found.push((l, by_symbol));
        }
    }
    if found.is_empty() {
        return Ok(None);
    }
    let binds = |(l, by_symbol): &(&Linked, bool)| matches!(l.kind, Kind::Rule | Kind::Decision) || *by_symbol;
    let files: Vec<String> = targets.iter().filter_map(|t| relative(&folders, &t.file).map(|r| r.1)).collect();
    // everything found counts as shown: the next edit here goes through
    let ids: Vec<String> = found.iter().map(|f| f.0.id.clone()).collect();
    state.shown(&ids, false)?;
    if !found.iter().any(binds) {
        // only the file's past errors and notes: after the edit, no turn lost
        state.event("after-edit", &files.join(" "), &ids.join(" "))?;
        return Ok(None);
    }
    Ok(Some(format!(
        "Not a refusal: Paragma holds memory about the code this edit touches ({}), shown once before the first edit there:\n{}\nIf the edit still fits, make it again unchanged; otherwise change it to fit.",
        files.join(", "),
        listing(project, &index, found)?.join("\n")
    )))
}

/// The notes found, most binding first (rules, decisions, what names the
/// changed function, then the newest), notes saying the same in one line,
/// at most JUST_IN_TIME lines.
fn listing(project: &crate::project::Project, index: &Index, mut found: Vec<(&Linked, bool)>) -> Result<Vec<String>> {
    let rank = |(l, by_symbol): &(&Linked, bool)| {
        let kind = match l.kind {
            Kind::Rule => 0,
            Kind::Decision => 1,
            _ if *by_symbol => 2,
            _ => 3,
        };
        let number: i64 = l.id.trim_start_matches(|c: char| c.is_alphabetic()).parse().unwrap_or(0);
        (kind, -number)
    };
    found.sort_by_key(rank);
    let mut lines: Vec<(Vec<String>, String)> = Vec::new(); // ids, what they say
    for (l, _) in &found {
        let line = describe(project, index, l)?;
        // the same words under another id: one line, both ids
        let said = line.replacen(&l.id, "", 1).replace(&format!("(pg read {})", l.id), "");
        match lines.iter_mut().find(|(_, s)| *s == said) {
            Some((ids, _)) => ids.push(l.id.clone()),
            None => lines.push((vec![l.id.clone()], said)),
        }
    }
    let more = lines.len().saturating_sub(JUST_IN_TIME);
    let mut out: Vec<String> = lines
        .into_iter()
        .take(JUST_IN_TIME)
        .map(|(ids, said)| {
            let first = &ids[0];
            let line = format!("{}{}", ids.join(", "), said.trim_end());
            if line.contains("(pg read") || !said.contains(": ") || ids.len() == 1 && said.starts_with(" rule") {
                line
            } else {
                format!("{line} (pg read {first})")
            }
        })
        .map(|l| l.replace("  ", " "))
        .collect();
    if more > 0 {
        out.push(format!("+{more} more about this code: pg tree <file>"));
    }
    Ok(out)
}

/// After an edit: the notes found before it that did not bind it (the
/// file's past errors), now that it is made.
fn after_edit(project: &crate::project::Project, state: &State) -> Result<Option<String>> {
    let Some((files, ids)) = state.take_after_edit()? else { return Ok(None) };
    let index = Index::open(project)?;
    let notes = CodeNotes::load(&index)?;
    let found: Vec<(&Linked, bool)> = notes.linked().filter(|l| ids.contains(&l.id)).map(|l| (l, false)).collect();
    if found.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "Paragma: memory about the code just changed ({files}), in case it bears on the change:\n{}",
        listing(project, &index, found)?.join("\n")
    )))
}

/// One line about a linked note: an error with its fix, the others with
/// where to read them.
fn describe(project: &crate::project::Project, index: &Index, l: &Linked) -> Result<String> {
    let text = one_line(&l.text, 200);
    if l.kind == Kind::Rule {
        return Ok(format!("{} rule: {text}", l.id));
    }
    // what the note says, not only its title: an error's fix, a decision's
    // or note's first lines
    let more = match index.note_by_ref(&l.id)? {
        Some(row) => {
            let note = Note::parse(&files::read_text(&project.dir.join(&row.path)).unwrap_or_default());
            match l.kind {
                Kind::Error => note.section("Fix").map(|f| format!("; fix: {}", one_line(&f, 200))),
                _ => Some(note.body.trim()).filter(|b| !b.is_empty()).map(|b| format!(": {}", one_line(b, 240))),
            }
            .unwrap_or_default()
        }
        None => String::new(),
    };
    Ok(format!("{} {}: {text}{more} (pg read {})", l.id, l.kind.as_str(), l.id))
}

/// A file an edit is about to change: as it is now, and the lines the edit
/// touches in it (None: all of it).
struct Target {
    file: PathBuf,
    source: String,
    lines: Option<Vec<usize>>,
}

/// What an edit is about to change. Claude Code's Edit and MultiEdit give
/// the text they replace, Write the whole file; Codex's apply_patch gives a
/// patch, whose removed lines (or, for a pure addition, the line before
/// it) are found in the file.
fn edit_targets(tool: &str, input: &Value, cwd: &Path) -> Vec<Target> {
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
    let abs = |p: &str| {
        let p = PathBuf::from(p);
        if p.is_absolute() { p } else { cwd.join(p) }
    };
    match tool {
        "Edit" | "MultiEdit" => {
            let Some(path) = input["file_path"].as_str() else { return Vec::new() };
            let file = abs(path);
            let source = read(&file);
            let mut edits: Vec<(&str, bool)> = Vec::new();
            if let Some(old) = input["old_string"].as_str() {
                edits.push((old, input["replace_all"].as_bool() == Some(true)));
            }
            for e in input["edits"].as_array().into_iter().flatten() {
                if let Some(old) = e["old_string"].as_str() {
                    edits.push((old, e["replace_all"].as_bool() == Some(true)));
                }
            }
            let mut lines = Vec::new();
            for (old, all) in edits {
                lines.extend(lines_of(&source, old, all));
            }
            vec![Target { file, source, lines: Some(lines) }]
        }
        "Write" => {
            let Some(path) = input["file_path"].as_str() else { return Vec::new() };
            let file = abs(path);
            let source = read(&file);
            vec![Target { file, source, lines: None }]
        }
        "apply_patch" => {
            let patch = input["command"].as_str().or_else(|| input["input"].as_str()).unwrap_or("");
            let mut out: Vec<Target> = Vec::new();
            let mut anchors: Vec<String> = Vec::new();
            let mut context: Option<String> = None;
            let mut removed = false;
            let finish = |out: &mut Vec<Target>, anchors: &mut Vec<String>| {
                if let Some(t) = out.last_mut() {
                    t.lines = Some(find_lines(&t.source, anchors));
                }
                anchors.clear();
            };
            for line in patch.lines() {
                if let Some(p) = line.strip_prefix("*** Update File: ") {
                    finish(&mut out, &mut anchors);
                    let file = abs(p.trim());
                    let source = read(&file);
                    out.push(Target { file, source, lines: Some(Vec::new()) });
                } else if let Some(p) =
                    line.strip_prefix("*** Add File: ").or_else(|| line.strip_prefix("*** Delete File: "))
                {
                    finish(&mut out, &mut anchors);
                    let file = abs(p.trim());
                    let source = read(&file);
                    out.push(Target { file, source, lines: Some(Vec::new()) });
                    anchors.clear();
                } else if line.starts_with("***") {
                    // End Patch, Move to: nothing to find
                } else if let Some(h) = line.strip_prefix("@@") {
                    context = Some(h.trim().to_string()).filter(|h| !h.is_empty());
                    removed = false;
                } else if let Some(r) = line.strip_prefix('-') {
                    anchors.push(r.to_string());
                    removed = true;
                } else if line.starts_with('+') {
                    if !removed && let Some(c) = context.take() {
                        anchors.push(c);
                    }
                } else if let Some(c) = line.strip_prefix(' ') {
                    context = Some(c.to_string());
                    removed = false;
                }
            }
            finish(&mut out, &mut anchors);
            out
        }
        _ => Vec::new(),
    }
}

/// The lines (1-based) the text `old` covers where it stands in `source`:
/// the first place, or every place with `all`. None found: no lines.
fn lines_of(source: &str, old: &str, all: bool) -> Vec<usize> {
    let mut lines = Vec::new();
    if old.is_empty() {
        return lines;
    }
    // a file with Windows line endings, an edit written with \n
    let crlf;
    let old = if !source.contains(old) && source.contains("\r\n") {
        crlf = old.replace('\n', "\r\n");
        crlf.as_str()
    } else {
        old
    };
    for (at, _) in source.match_indices(old) {
        let first = source[..at].matches('\n').count() + 1;
        let last = first + old.trim_end_matches(['\n', '\r']).matches('\n').count();
        lines.extend(first..=last);
        if !all {
            break;
        }
    }
    lines
}

/// The lines of `source` holding the given lines of a patch, in order: each
/// is looked for after the one before.
fn find_lines(source: &str, wanted: &[String]) -> Vec<usize> {
    let file: Vec<&str> = source.lines().collect();
    let mut from = 0;
    let mut found = Vec::new();
    for w in wanted {
        let w = w.trim();
        if w.is_empty() {
            continue;
        }
        if let Some(i) = (from..file.len()).find(|&i| file[i].trim() == w) {
            found.push(i + 1);
            from = i + 1;
        }
    }
    found
}

/// A file as (the code folder it is in, its path there), when it is in one
/// of the project's folders.
fn relative(folders: &[String], file: &Path) -> Option<(String, String)> {
    let key = folder_key(file);
    let original =
        std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf()).to_string_lossy().replace('\\', "/");
    folders.iter().find_map(|f| {
        let rest = key.strip_prefix(f.as_str())?.strip_prefix('/')?;
        // the path as written (the key is lower case on Windows)
        let tail = original.get(original.len() - rest.len()..).filter(|t| t.eq_ignore_ascii_case(rest)).unwrap_or(rest);
        Some((f.clone(), tail.to_string()))
    })
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
        return after_edit(project, &state);
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
    let failed = !parsed.failed.is_empty() || parse::suite_failed(&output);
    let tests = failed || parsed.passed > 0 || parse::suite_passed(&output);
    let exit = match exit {
        Some(code) => code,
        // no exit code given: only a test or build tool's own words count,
        // not lines that look like errors (a source file printed)
        None if !tests => return Ok(None),
        None => i32::from(failed),
    };
    if !tests && (exit == 0 || parsed.errors.is_empty()) {
        return Ok(None); // not a test run or a build that failed
    }
    // one history for `go test ./...`, `cd app && go test ./... 2>&1 | head`
    let command = &testrun::normalized(command);
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
    let index = Index::open(project)?;
    if !verdict.fixed.is_empty() {
        // the fixes shown for these tests helped (state/outcome.rs)
        state.tests_fixed(&run.command, &verdict.fixed)?;
    }
    if exit == 0 {
        return fixed_here(cx, project, (&state, &index, code.as_ref()), &verdict, history.first());
    }

    let mut lines = Vec::new();
    if !verdict.flaky.is_empty() {
        lines.push(format!("flaky (fails now and then, also without a change): {}", verdict.flaky.join(", ")));
    }
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
        if let Some(own) = super::run::known_id(&k) {
            state.fix_shown(own, &run.command, &run.failing)?;
        }
        lines.push(format!("{line}\n  known: {k}"));
    }
    if lines.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!("Paragma, about this run:\n{}", lines.join("\n"))))
}

/// Tests that failed in the last run pass now: when memory has no fix for
/// the error they failed with, the agent is asked to remember it, while it
/// still knows the cause. Nothing is written for it.
fn fixed_here(
    cx: &Context,
    project: &crate::project::Project,
    (state, index, code): (&State, &Index, Option<&crate::code::CodeIndex>),
    verdict: &testrun::Verdict,
    before: Option<&Run>,
) -> Result<Option<String>> {
    let (Some(before), false) = (before, verdict.fixed.is_empty()) else { return Ok(None) };
    let Some(line) = before.errors.first() else { return Ok(None) };
    if super::run::known(index, project, line)?.is_some() {
        return Ok(None);
    }
    state.event("fixed", line, &verdict.fixed.join(", "))?;
    let folders = Links::load(&cx.home).folders_of(&project.name);
    let changed: Vec<String> = state
        .my_targets_since("edit", before.at)?
        .iter()
        .filter_map(|f| relative(&folders, Path::new(f)).map(|r| r.1))
        .collect();
    let (since, files) = if changed.is_empty() {
        (String::new(), String::new())
    } else {
        // the record is then tied to the code that was fixed, not only to the test
        let mut files: Vec<String> = changed.clone();
        if let Some(code) = code {
            for (file, _) in parse::locations(line) {
                if let Some((folder, path)) = code.locate(&file)? {
                    files.push(code.display(&folder, &path));
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        files.retain(|f| seen.insert(f.clone()));
        (format!(" Changed since: {}.", changed.join(", ")), format!(" --files {}", files.join(",")))
    };
    Ok(Some(format!(
        "Paragma: {} pass again; they failed with \"{}\" and memory has no fix for it.{since} If the cause is worth \
         knowing next time: pg write --error {:?} --cause \"...\" --fix \"...\"{files}",
        verdict.fixed.join(", "),
        one_line(line, 160),
        one_line(line, 160)
    )))
}

/// The user's message gives the project a lasting rule: the agent is asked
/// to keep it, if the user meant it so (record/said_rule.rs). Each sentence
/// once a conversation.
/// What the user's message holds that memory should keep: a lasting rule,
/// decisions or a plan of steps.
fn user_said(project: &crate::project::Project, prompt: &str) -> Result<Option<String>> {
    let parts: Vec<String> = [said_plan(project, prompt)?, said_rule(project, prompt)?].into_iter().flatten().collect();
    Ok((!parts.is_empty()).then(|| {
        parts.join(
            "
",
        )
    }))
}

/// Decisions or a plan for work that goes on after this conversation:
/// asks once a conversation to record them first, in the user's words.
fn said_plan(project: &crate::project::Project, prompt: &str) -> Result<Option<String>> {
    if !crate::record::plan::gives_plan(prompt) {
        return Ok(None);
    }
    let state = State::open(project)?;
    if state.my_last("plan-offered").is_some() {
        return Ok(None);
    }
    state.event("plan-offered", &project.name, "")?;
    Ok(Some(
        "Paragma: the user's message gives decisions or a plan. Before the work, record them in the user's words:          pg open --goal \"...\" --step \"...\" --decision \"...\" (one --step per step, also the ones for later; one          --decision per decision). A later conversation knows only what is recorded."
            .to_string(),
    ))
}

fn said_rule(project: &crate::project::Project, prompt: &str) -> Result<Option<String>> {
    let Some(sentence) = crate::record::said_rule::find(prompt) else { return Ok(None) };
    let sentence = one_line(&sentence, 300);
    let state = State::open(project)?;
    if state.my_targets_since("rule-offered", 0)?.contains(&sentence) {
        return Ok(None);
    }
    state.event("rule-offered", &sentence, "")?;
    Ok(Some(format!(
        "Paragma: the user's message reads like a lasting rule for this project: \"{sentence}\". If they mean it for \
         the future, keep it: pg write --rule \"<the rule in a sentence>\" (add (applies: <folder>/) when it is about \
         one part of the code). If it was only for this task, ignore this."
    )))
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
    // a failed command does not always come as PostToolUseFailure: Claude
    // Code gave `go test` failing with exit code 1 as a plain result
    let exit = exit
        .or_else(|| exit_in(&text))
        .or((event["hook_event_name"].as_str() == Some("PostToolUseFailure")).then_some(1));
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
        // Claude Code 2.1: a plain result says no exit code, even for `go test`
        // failing with 1 (as it came in a real run)
        let claude_ok = json!({ "hook_event_name": "PostToolUse", "tool_response": { "stdout": "ok", "stderr": "" } });
        assert_eq!(tool_output(&claude_ok).1, None);
        let claude_failed = json!({ "hook_event_name": "PostToolUse", "tool_response": { "stdout": "--- FAIL: TestRefundRejectsNegativeDays (0.00s)\n    refund_test.go:7: negative days must be rejected\nFAIL\nFAIL\tapp/payments\t3.460s\nFAIL", "stderr": "", "interrupted": false, "isImage": false, "noOutputExpected": false } });
        assert_eq!(tool_output(&claude_failed).1, None);
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
    fn the_lines_an_edit_touches() {
        let src = "package p\n\nfunc A() {\n\tx := 1\n\ty := 2\n}\n\nfunc B() {\n\tx := 1\n}\n";
        assert_eq!(lines_of(src, "\tx := 1\n\ty := 2", false), [4, 5]);
        assert_eq!(lines_of(src, "\tx := 1\n", false), [4]);
        assert_eq!(lines_of(src, "\tx := 1", true), [4, 9]);
        assert!(lines_of(src, "nowhere", false).is_empty());
        let crlf = src.replace('\n', "\r\n");
        assert_eq!(lines_of(&crlf, "\tx := 1\n\ty := 2", false), [4, 5]);
        // a patch's lines are found in order, each after the one before
        let wanted = ["func B() {".to_string(), "x := 1".to_string()];
        assert_eq!(find_lines(src, &wanted), [8, 9]);
    }

    #[test]
    fn a_patch_names_its_files_and_lines() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.go"), "package p\n\nfunc A() {\n\told()\n}\n\nfunc B() {\n}\n").unwrap();
        let patch = "*** Begin Patch\n*** Update File: a.go\n@@ func A() {\n-\told()\n+\tnew()\n@@\n func B() {\n+\tmore()\n*** Add File: b.go\n+package p\n*** End Patch\n";
        let t = edit_targets("apply_patch", &json!({ "command": patch }), d.path());
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].file, d.path().join("a.go"));
        assert_eq!(t[0].lines.as_deref(), Some(&[4, 7][..]));
        assert_eq!(t[1].file, d.path().join("b.go"));
        assert_eq!(t[1].lines.as_deref(), Some(&[][..]));
        let w = edit_targets("Write", &json!({ "file_path": d.path().join("a.go").to_string_lossy() }), d.path());
        assert_eq!(w[0].lines, None);
    }

    #[test]
    fn files_inside_the_projects_folders() {
        let d = tempfile::tempdir().unwrap();
        let shop = d.path().join("Shop");
        let key = folder_key(&shop);
        let (folder, rel) = relative(std::slice::from_ref(&key), &shop.join("Payments").join("Refund.go")).unwrap();
        assert_eq!(folder, key);
        assert_eq!(rel, "Payments/Refund.go");
        assert_eq!(relative(&[key], &d.path().join("ShopX").join("a.go")), None);
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
