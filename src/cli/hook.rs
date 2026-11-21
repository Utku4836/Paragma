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
//! UserPromptSubmit        what this conversation's work calls for: a plan
//!                         recorded, a handoff kept (from what it did, never
//!                         from the words of the message)
//! Stop                    work done but no handoff: asks once for pg finish
//! ```

use super::Context;
use crate::code::notes::{CodeNotes, Linked, about_lines_by};
use crate::index::Index;
use crate::note::{Kind, Note};
use crate::output::{now, one_line};
use crate::project::files;
use crate::state::journal::Did;
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
    // the start brings in what other computers sent; the end sends
    if matches!(event_name(&input).as_str(), "SessionStart" | "Stop") {
        crate::sync::trigger(&crate::project::Home::locate());
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
        "UserPromptSubmit" => user_said(project)?,
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
    let brief = super::open::brief_at_start(cx, project, source == "compact")?;
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
    // another conversation still at work changed the same file: said once
    let mut conflicts = Vec::new();
    let paths: Vec<String> = targets.iter().map(|t| t.file.to_string_lossy().into_owned()).collect();
    for (key, line) in super::act::others_editing(&state, &paths)? {
        if !state.shown_to_me(&key, 0)? {
            state.shown(std::slice::from_ref(&key))?;
            conflicts.push(line);
        }
    }
    let conflict = (!conflicts.is_empty()).then(|| {
        format!(
            "Not a refusal: Paragma: {}. Check with it first (pg tell \"...\" reaches it; pg steps shows the step it took), \
             so neither undoes the other's change. If the edit still fits, make it again unchanged.",
            conflicts.join("; ")
        )
    });
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
        return Ok(conflict);
    }
    let binds = |(l, by_symbol): &(&Linked, bool)| matches!(l.kind, Kind::Rule | Kind::Decision) || *by_symbol;
    let files: Vec<String> = targets.iter().filter_map(|t| relative(&folders, &t.file).map(|r| r.1)).collect();
    // everything found counts as shown: the next edit here goes through
    let ids: Vec<String> = found.iter().map(|f| f.0.id.clone()).collect();
    state.shown(&ids)?;
    if !found.iter().any(binds) {
        // only the file's past errors and notes: after the edit, no turn lost
        state.record(Did::AfterEdit { files: files.join(" "), ids: ids.clone() })?;
        return Ok(conflict);
    }
    let mut text = format!(
        "Not a refusal: Paragma holds memory about the code this edit touches ({}), shown once before the first edit there:\n{}\nIf the edit still fits, make it again unchanged; otherwise change it to fit.",
        files.join(", "),
        listing(project, &index, found)?.join("\n")
    );
    if let Some(c) = conflict {
        text.push_str(&format!("\nAlso: {}", c.trim_start_matches("Not a refusal: Paragma: ")));
    }
    Ok(Some(text))
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
        // as the folder has it: "PAYMENTS/Refund.go" is payments/refund.go there
        Some((f.clone(), if cfg!(windows) { real_case(Path::new(f), tail) } else { tail.to_string() }))
    })
}

/// A path under `dir` in the letter case its files have (Windows ignores
/// case; git and pg's index do not). As given where a part is not there.
fn real_case(dir: &Path, rel: &str) -> String {
    let mut at = dir.to_path_buf();
    let mut out: Vec<String> = Vec::new();
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        let found = std::fs::read_dir(&at).ok().and_then(|es| {
            es.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).find(|n| n.eq_ignore_ascii_case(part))
        });
        let name = found.unwrap_or_else(|| part.to_string());
        at.push(&name);
        out.push(name);
    }
    out.join("/")
}

/// After a tool: an edit is remembered, a failing test run is explained.
fn after_tool(cx: &Context, project: &crate::project::Project, event: &Value) -> Result<Option<String>> {
    let tool = event["tool_name"].as_str().unwrap_or("");
    let edited = edited_files(tool, &event["tool_input"]);
    if !edited.is_empty() {
        let state = State::open(project)?;
        let linked = crate::state::links::Links::load(&cx.home).folders_of(&project.name);
        for f in &edited {
            // as the code folder has it ("payments/refund.go"), not where it is on this disk
            let file = relative(&linked, &cx.cwd.join(f)).map(|(_, p)| p).unwrap_or_else(|| f.to_string());
            state.record(Did::Edit { file })?;
        }
        let mut text = after_edit(project, &state)?;
        // the edit changed a function's shape and left its callers as they were
        if let Some(code) = crate::code::notes::open(&cx.home, project)? {
            let folders = crate::state::links::Links::load(&cx.home).folders_of(&project.name);
            let files: Vec<(String, String)> =
                edited.iter().filter_map(|f| relative(&folders, &cx.cwd.join(f))).collect();
            let left = super::ripple::warnings(project, &code, Some(&files), false);
            if !left.is_empty() {
                let said = format!("Paragma: this change leaves code that calls it as it was:\n{}", left.join("\n"));
                text = Some(match text {
                    Some(t) => format!("{t}\n{said}"),
                    None => said,
                });
            }
        }
        return Ok(text);
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
    crate::code::git::changed_ahead(Links::load(&cx.home).folders_of(&project.name));
    let code = crate::code::notes::open(&cx.home, project)?;
    let run = Run {
        at: now(),
        command: command.to_string(),
        exit,
        seconds: event["duration_ms"].as_f64().unwrap_or(0.0) / 1000.0,
        failing: parsed.failed.clone(),
        passed: parse::counts(&output).map(|c| c.passed).unwrap_or(parsed.passed),
        errors: parsed.errors.clone(),
        changed: code.as_ref().map(super::run::changed_files).unwrap_or_default(),
        symbols: code.as_ref().map(super::run::changed_symbols).unwrap_or_default(),
        code: code.as_ref().map(super::run::code_state).unwrap_or_default(),
    };
    let state = State::open(project)?;
    let history = state.runs(Some(&run.command), 10)?;
    let verdict = testrun::judge(&run, &history);
    state.record_run(&run)?;
    state.record(Did::Run { command: run.command.clone(), passed: exit == 0, failing: run.failing.len() })?;
    let index = Index::open(project)?;
    if !verdict.fixed.is_empty() {
        // the fixes shown for these tests helped (state/outcome.rs)
        state.tests_fixed(&run.command, &verdict.fixed)?;
    }
    if exit == 0 {
        return fixed_here(cx, project, (&state, &index, code.as_ref()), &verdict, history.first(), &run.symbols);
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
        state.shown(std::slice::from_ref(&id))?;
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
    now: &[String],
) -> Result<Option<String>> {
    let (Some(before), false) = (before, verdict.fixed.is_empty()) else { return Ok(None) };
    let Some(line) = before.errors.first() else { return Ok(None) };
    if super::run::known(index, project, line)?.is_some() {
        return Ok(None);
    }
    state.record(Did::Fixed { line: line.to_string(), tests: verdict.fixed.clone() })?;
    let folders = Links::load(&cx.home).folders_of(&project.name);
    let changed: Vec<String> = state
        .my_targets_since("edit", before.at)?
        .iter()
        .filter_map(|f| relative(&folders, Path::new(f)).map(|r| r.1))
        .collect();
    // the functions changed after the failing run, when it kept them (kernel)
    let kept = !before.symbols.is_empty() || before.changed.is_empty();
    let functions =
        crate::kernel::experts::fix_changes(kept.then_some(before.symbols.as_slice()), now).filter(|f| !f.is_empty());
    let changed = match &functions {
        Some(f) => f.clone(),
        None => changed,
    };
    let (since, files) = if changed.is_empty() {
        (String::new(), String::new())
    } else {
        // the record is then tied to the code that was fixed, not only to the test
        let mut files: Vec<String> = changed
            .iter()
            .map(|c| c.split_once('#').map(|(f, _)| f.to_string()).unwrap_or_else(|| c.clone()))
            .collect();
        if let Some(code) = code {
            for (file, _) in parse::locations(line) {
                if let Some((folder, path)) = code.locate(&file)? {
                    files.push(code.display(&folder, &path));
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        files.retain(|f| seen.insert(f.clone()));
        // pg keeps it as a draft by itself (cli/act.rs): the agent says why
        let shown: Vec<String> = changed.iter().take(5).cloned().collect();
        if let Some(said) = super::act::draft_fix(project, state, line, &shown, &files)? {
            return Ok(Some(format!(
                "Paragma: {} pass again; they failed with \"{}\". {said}",
                verdict.fixed.join(", "),
                one_line(line, 160)
            )));
        }
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

/// At the user's message: what this conversation's work calls for (a
/// plan recorded, a handoff kept) and notes from the others, once each.
/// The message itself is not read: what is done tells it, in any language.
fn user_said(project: &crate::project::Project) -> Result<Option<String>> {
    let state = State::open(project)?;
    let lines = super::perceive::news(project, &state)?;
    Ok((!lines.is_empty()).then(|| {
        lines.iter().map(|l| format!("Paragma: {l}")).collect::<Vec<_>>().join(
            "
",
        )
    }))
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
    state.record(Did::AskedFinish)?;
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

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/cli/hook.rs"]
mod tests;
