//! pg run: run the tests or a build and say what the result means.
//!
//! ```text
//! pg run -- go test ./...
//! ```
//!
//! Instead of the whole output: which tests fail, whether each failure is
//! new, was failing already or comes and goes, the lines that say what went
//! wrong with the function they point at, and the fix memory already has
//! for that error. Exits with the command's exit code.
//!
//! The project's secrets are given to the command: $NAME in the command line
//! becomes the value (on every system), every secret is in its environment,
//! and a value showing up in the output is shown as $NAME again.

use super::Context;
use crate::code::notes;
use crate::code::{CodeIndex, git};
use crate::index::Index;
use crate::note::Note;
use crate::output::{Budget, count, now, one_line};
use crate::project::files;
use crate::record;
use crate::state::State;
use crate::state::journal::Did;
use crate::testrun::{self, Run, parse};
use anyhow::{Context as _, Result};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(clap::Args)]
pub struct Args {
    /// Stop the command (and what it started) after this many seconds
    #[arg(long, value_name = "SECONDS", default_value_t = 600)]
    timeout: u64,

    /// The result as JSON: the tool's own counts, the failing tests (new,
    /// already, flaky), fixed ones, error lines, this run's log file, and
    /// what pg says as lines
    #[arg(long)]
    json: bool,

    /// The command, after --: pg run -- go test ./...
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true, value_name = "COMMAND")]
    command: Vec<String>,
}

/// Output lines kept: the first ones and, mostly, the last ones (where test
/// tools put their summary).
const HEAD_LINES: usize = 2_000;
const TAIL_LINES: usize = 20_000;
/// The exit code of a command stopped at its timeout (as timeout(1) gives).
const TIMED_OUT: i32 = 124;

/// Earlier runs of a command compared with.
const HISTORY: usize = 10;
/// Error lines explained.
const EXPLAINED: usize = 3;
/// An output longer than this many lines is named, with where its errors are.
const WHOLE_OUTPUT: usize = 20;
/// Runs whose whole output is kept, each in its own file (`.pg/runs/`).
const RUN_LOGS: usize = 20;

/// This run's whole output, in a file of its own: a path said for an
/// earlier run still shows that run.
struct Log {
    path: Option<std::path::PathBuf>,
    written: bool,
}

impl Log {
    fn new(project: Option<&crate::project::Project>) -> Self {
        let path = project.map(|p| {
            let stamp = stamp();
            p.meta_dir().join("runs").join(format!("run-{stamp}-{}.log", std::process::id()))
        });
        Log { path, written: false }
    }

    /// Writes it (once), leaving the newest RUN_LOGS: its path.
    fn keep(&mut self, output: &str) -> Option<&std::path::Path> {
        let path = self.path.as_ref()?;
        if !self.written {
            let dir = path.parent()?;
            std::fs::create_dir_all(dir).ok()?;
            let mut old: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("run-") && n.ends_with(".log"))
                })
                .collect();
            old.sort();
            while old.len() >= RUN_LOGS {
                let _ = std::fs::remove_file(old.remove(0));
            }
            std::fs::write(path, output).ok()?;
            self.written = true;
        }
        Some(path)
    }
}

/// "20261121-143005" (UTC), sortable as text.
fn stamp() -> String {
    let t = now();
    let s = t.rem_euclid(86_400);
    format!("{}-{:02}{:02}{:02}", crate::output::date(t).replace('-', ""), s / 3600, s % 3600 / 60, s % 60)
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let command = join(&a.command);
    let started = Instant::now();
    let secrets = cx.project.as_ref().map(crate::project::env::load).unwrap_or_default();
    let line = crate::project::env::substitute(&command, &secrets);
    crate::output::mark("run: secrets");
    let (exit, output, timed_out) = execute(&line, &cx.cwd, &secrets, a.timeout)?;
    let output = crate::project::env::hide(&output, &secrets);
    // as shown and kept in the history of runs: without any value typed in it
    let command = crate::project::env::hide(&command, &secrets);
    let parsed = parse::parse(&output);
    // the numbers the test tool said itself; else the lines pg read
    let counts = parse::counts(&output);
    crate::output::mark("run: command done");
    // the command may have made or removed files
    crate::code::files::forget();
    if let Some(p) = &cx.project {
        crate::code::git::changed_ahead(crate::state::links::Links::load(&cx.home).folders_of(&p.name));
    }
    let code = match &cx.project {
        Some(p) => notes::open(&cx.home, p)?,
        None => None,
    };
    let run = Run {
        at: now(),
        command: command.clone(),
        exit,
        seconds: started.elapsed().as_secs_f64(),
        failing: parsed.failed.clone(),
        passed: counts.as_ref().map(|c| c.passed).unwrap_or(parsed.passed),
        errors: parsed.errors.clone(),
        changed: code.as_ref().map(changed_files).unwrap_or_default(),
        symbols: code.as_ref().map(changed_symbols).unwrap_or_default(),
        code: code.as_ref().map(code_state).unwrap_or_default(),
    };
    crate::output::mark("run: code index and changes");

    let mut b = Budget::new(if a.json { usize::MAX / 4 } else { cx.budget });
    let mut log = Log::new(cx.project.as_ref());
    let tests = tests_said(counts.as_ref(), &run);
    if timed_out {
        b.add(&format!("fail {command}: stopped after {} s (--timeout), with what it started{tests}", a.timeout));
    } else if exit == 0 {
        b.add(&format!("ok {command} ({:.1} s){tests}", run.seconds));
    } else {
        b.add(&format!("fail {command} (exit {exit}, {:.1} s){tests}", run.seconds));
    }

    let Some(project) = &cx.project else {
        tail(&mut b, &output, if exit == 0 { 3 } else { 40 }, &mut log);
        b.add("(no project here: pg open keeps the history of runs and finds known fixes)");
        if a.json {
            print_json(&run, counts.as_ref(), &Default::default(), timed_out, None, &b);
            exit_with(exit);
        }
        finish(&b, exit);
    };
    let state = State::open(project)?;
    let history = state.runs(Some(&command), HISTORY)?;
    let verdict = testrun::judge(&run, &history);
    let list = |v: &[String]| v.join(", ");
    if !verdict.new.is_empty() {
        b.add(&format!("new: {}", list(&verdict.new)));
    }
    if !verdict.already.is_empty() {
        let v: Vec<String> = verdict.already.iter().map(|(t, n)| format!("{t} ({n} runs)")).collect();
        b.add(&format!("failing already: {}", v.join(", ")));
    }
    if !verdict.flaky.is_empty() {
        b.add(&format!("flaky (fails now and then): {}", list(&verdict.flaky)));
    }
    if !verdict.fixed.is_empty() {
        b.add(&format!("fixed since the last run: {}", list(&verdict.fixed)));
        state.tests_fixed(&command, &verdict.fixed)?;
    }

    let index = Index::open(project)?;
    let mut unexplained: Option<(String, Vec<String>)> = None;
    // the same errors as this conversation's last run of it: said already
    let previous = state.my_last_run()?.filter(|p| p.command == command && p.exit != 0);
    let signatures = |errors: &[String]| -> Vec<String> {
        let mut v: Vec<String> = errors.iter().map(|e| record::signature::normalize(e)).collect();
        v.sort();
        v
    };
    let said = state.said()?;
    // ...unless memory has a fix for one of them not shown yet
    let news = run.errors.iter().take(EXPLAINED).any(|line| {
        known(&index, project, line)
            .ok()
            .flatten()
            .and_then(|k| known_id(&k).map(|id| !said.contains(&format!("known:{id}"))))
            .unwrap_or(false)
    });
    let same_as_before = exit != 0
        && !run.errors.is_empty()
        && !news
        && previous.as_ref().is_some_and(|p| signatures(&p.errors) == signatures(&run.errors));
    let mut places: Vec<usize> = Vec::new();
    if same_as_before {
        b.add(&format!(
            "errors: the same {} as the last run here (first: {})",
            run.errors.len(),
            one_line(&run.errors[0], 100)
        ));
    } else if exit != 0 && !run.errors.is_empty() {
        b.add("errors:");
        // lines already said as part of an error above are not errors again
        let mut told: Vec<String> = Vec::new();
        for line in run.errors.iter().take(EXPLAINED) {
            if told.iter().any(|t| t.trim() == line.trim()) {
                continue;
            }
            b.add(line);
            // what explains it: the values compared, its own frames
            let (at, details) = testrun::explain::context(&output, line);
            places.extend(at);
            let mut files_hit = Vec::new();
            if let Some(code) = &code {
                for (file, n) in parse::locations(line) {
                    if let Some((folder, path)) = code.locate(&file)? {
                        files_hit.push(code.display(&folder, &path));
                        if let Some(s) = code.at(&folder, &path, n)? {
                            b.add(&format!(
                                "  in {}#{} ({} {}-{})",
                                code.display(&folder, &path),
                                s.qualified,
                                s.kind,
                                s.start,
                                s.end
                            ));
                        }
                    }
                }
            }
            for d in details {
                b.add(&format!("    {d}"));
                told.push(d);
            }
            let known = match known(&index, project, line)? {
                Some(k) => Some(k),
                None => known_elsewhere(cx, project, line)?,
            };
            match known {
                // its fix in full once a conversation; after that by its id
                Some(k) if known_id(&k).is_some_and(|id| said.contains(&format!("known:{id}"))) => {
                    b.add(&format!("  known: {} (its fix was shown above)", known_id(&k).unwrap_or("")));
                }
                Some(k) => {
                    b.add(&format!("  known: {k}"));
                    if let Some(id) = known_id(&k) {
                        state.fix_shown(id, &command, &run.failing)?;
                        state.say(&[format!("known:{id}")])?;
                    }
                }
                None if unexplained.is_none() => unexplained = Some((line.clone(), files_hit)),
                None => {}
            }
        }
    }
    if exit != 0 {
        // a name the build looked for in a library: what the library has
        for h in crate::libs::hints(&cx.home, &cx.code_dirs(project), &output).unwrap_or_default() {
            b.add(&format!("library: {h}"));
        }
    }
    // a change that left callers as they were, passing or not (pg ripple)
    if let Some(c) = &code {
        for w in super::ripple::warnings(project, c, None, false) {
            b.add(&format!("ripple: {w}"));
        }
    }
    // the whole output, for the part around an error (a short one was all said)
    // (the same errors as before: named with them already)
    if exit != 0
        && !run.errors.is_empty()
        && !same_as_before
        && output.lines().filter(|l| !l.trim().is_empty()).count() > WHOLE_OUTPUT
    {
        if let Some(path) = log.keep(&output) {
            let lines = places.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
            let at = match places.len() {
                0 => String::new(),
                1 => format!("; the error at line {lines}"),
                _ => format!("; the errors at lines {lines}"),
            };
            b.add(&format!("(whole output: {}{at})", path.display()));
        }
    }
    let tests_seen = run.passed > 0 || !run.failing.is_empty();
    if exit != 0 && run.errors.is_empty() {
        tail(&mut b, &output, 30, &mut log); // nothing understood: the output says it best
    } else if exit == 0 && !tests_seen {
        tail(&mut b, &output, 60, &mut log); // not a test run (an API call, a script): its output is the answer
    }
    if let Some((line, files)) = unexplained {
        let files = if files.is_empty() { String::new() } else { format!(" --files {}", files.join(",")) };
        b.add(&format!(
            "once fixed, remember it: pg write --error {:?} --cause \"...\" --fix \"...\"{files}",
            one_line(&line, 120)
        ));
    } else if !verdict.fixed.is_empty()
        && let Some(line) =
            history.first().and_then(|r| r.errors.iter().find(|l| record::says_something(l)).or(r.errors.first()))
        && known(&index, project, line)?.is_none()
    {
        // a draft: the error, and the change that fixed it; the agent says why.
        // The change is what changed after the failing run (kernel), when
        // that run kept its functions; else all changed since the commit
        let failed = history.first();
        let narrowed = failed.and_then(|f| {
            let kept = !f.symbols.is_empty() || f.changed.is_empty();
            crate::kernel::experts::fix_changes(kept.then_some(f.symbols.as_slice()), &run.symbols)
        });
        let changes: Vec<(String, String)> = match (&code, narrowed) {
            (Some(_), Some(names)) => names
                .into_iter()
                .map(|n| {
                    let file = n.split_once('#').map(|(f, _)| f.to_string()).unwrap_or_else(|| n.clone());
                    (n, file)
                })
                .collect(),
            // the functions it changed (a new file may be anything)
            (Some(c), None) => crate::code::work::changed(c)
                .unwrap_or_default()
                .into_iter()
                .filter(|ch| ch.symbol.is_some())
                .map(|ch| (ch.show(c), ch.path))
                .collect(),
            (None, _) => Vec::new(),
        };
        let mut files: Vec<String> = Vec::new();
        for (_, p) in &changes {
            if !files.contains(p) {
                files.push(p.clone());
            }
        }
        let shown: Vec<&str> = changes.iter().take(5).map(|(s, _)| s.as_str()).collect();
        if shown.is_empty() {
            b.add(&format!(
                "worth remembering? pg write --error {:?} --cause \"...\" --fix \"...\"",
                one_line(line, 120)
            ));
        } else {
            // pg keeps it as a draft by itself (cli/act.rs): the agent says why
            let names: Vec<String> = shown.iter().map(|s| s.to_string()).collect();
            let files: Vec<String> = files.iter().take(5).cloned().collect();
            match super::act::draft_fix(project, &state, line, &names, &files)? {
                Some(said) => b.add(&format!("fixed: {said}")),
                None => b.add(&format!(
                    "fixed: remember it (the change is filled in, say what was wrong and how): pg write --error {:?} --cause \"...\" --fix \"...: {}\" --files {}",
                    one_line(line, 120),
                    shown.join(", "),
                    files.join(",")
                )),
            };
        }
        state.record(Did::Fixed { line: line.clone(), tests: verdict.fixed.clone() })?;
    }

    state.record_run(&run)?;
    state.record(Did::Run { command: command.clone(), passed: exit == 0, failing: run.failing.len() })?;
    // a test that failed and passed with the same code: a draft record
    let recent: Vec<Run> = std::iter::once(run.clone()).chain(history.iter().cloned()).collect();
    if !crate::kernel::learn::flaky(&recent).is_empty() {
        let facts = super::perceive::facts(project, &state, false)?;
        for line in super::act::flaky_records(project, &state, &facts, &command)? {
            b.add(&line);
        }
    }
    for line in super::perceive::news(&state)? {
        b.add(&line);
    }
    if a.json {
        let path = log.keep(&output).map(|p| p.display().to_string());
        print_json(&run, counts.as_ref(), &verdict, timed_out, path, &b);
        exit_with(exit);
    }
    finish(&b, exit);
}

/// ": 2 failing, 40 passed, 1 skipped (node's count)": the test tool's own
/// numbers when it said them, else the lines pg read.
fn tests_said(counts: Option<&parse::Counts>, run: &Run) -> String {
    match counts {
        Some(c) if c.passed + c.failed + c.skipped > 0 => {
            let mut parts = Vec::new();
            if c.failed > 0 {
                parts.push(format!("{} failing", c.failed));
            }
            parts.push(format!("{} passed", c.passed));
            if c.skipped > 0 {
                parts.push(format!("{} skipped", c.skipped));
            }
            format!(": {} ({}'s count)", parts.join(", "), c.tool)
        }
        _ if run.failing.is_empty() && run.passed == 0 => String::new(),
        _ if run.failing.is_empty() => format!(": {} passed", run.passed),
        _ => format!(": {} failing, {} passed", run.failing.len(), run.passed),
    }
}

/// pg run --json: what a program reading it needs, the counts as the test
/// tool said them.
fn print_json(
    run: &Run,
    counts: Option<&parse::Counts>,
    v: &testrun::Verdict,
    timed_out: bool,
    log: Option<String>,
    b: &Budget,
) {
    let counts = match counts {
        Some(c) => serde_json::json!({"passed": c.passed, "failed": c.failed, "skipped": c.skipped, "from": c.tool}),
        None => {
            serde_json::json!({"passed": run.passed, "failed": run.failing.len(), "skipped": null, "from": "lines"})
        }
    };
    let out = serde_json::json!({
        "command": run.command,
        "exit": run.exit,
        "seconds": (run.seconds * 10.0).round() / 10.0,
        "timed_out": timed_out,
        "tests": counts,
        "failing": run.failing,
        "new": v.new,
        "failing_already": v.already.iter().map(|(t, n)| serde_json::json!({"test": t, "runs": n})).collect::<Vec<_>>(),
        "flaky": v.flaky,
        "fixed": v.fixed,
        "errors": run.errors,
        "log": log,
        "said": b.out.lines().collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
}

/// Prints and ends with the command's exit code.
fn finish(b: &Budget, exit: i32) -> ! {
    print!("{}", b.out);
    exit_with(exit)
}

fn exit_with(exit: i32) -> ! {
    let _ = std::io::stdout().flush();
    std::process::exit(if exit == 0 { 0 } else { exit.clamp(1, 255) });
}

/// The output made short (testrun/compact.rs): its errors wherever they
/// are, warnings grouped, repeats and progress lines counted, its end; as
/// much as fits. When some was left out, the whole output is kept in this
/// run's own file the agent can read.
fn tail(b: &mut Budget, output: &str, lines: usize, log: &mut Log) {
    let mut shown = testrun::compact::compact(output, lines);
    let total = output.lines().filter(|l| !l.trim().is_empty()).count();
    let short = shown.len() < total;
    while !shown.is_empty() && !b.add(&format!("output:\n{}", shown.join("\n"))) {
        shown.remove(0);
    }
    if short && let Some(path) = log.keep(output) {
        b.add(&format!("(all {total} lines: {})", path.display()));
    }
}

/// The note memory has about an error line, with its fix.
pub(super) fn known(index: &Index, project: &crate::project::Project, line: &str) -> Result<Option<String>> {
    let Some(path) = record::known_error(index, line)? else { return Ok(None) };
    let note = Note::parse(&files::read_text(&project.dir.join(&path))?);
    // a draft pg wrote (cli/act.rs): offered while young, marked as such
    let draft = match super::act::draft_state(&note) {
        Some(false) => return Ok(None),
        Some(true) => " (a draft pg wrote: the change that fixed it, no cause yet)",
        None => "",
    };
    let id = format!("{}{draft}", note.id().unwrap_or("?"));
    let seen =
        note.props.text("seen").and_then(|s| s.parse::<i64>().ok()).map(|n| format!(" (seen {})", count(n, "time")));
    // the fix is what the agent needs: in full, up to a short paragraph
    let fix = note.section("Fix").map(|f| one_line(&f, 300)).unwrap_or_else(|| one_line(&note.body, 300));
    // how the fix did when it was shown before (state/outcome.rs)
    let track = State::open(project).and_then(|s| s.track(&id)).ok().and_then(|t| t.describe());
    let track = track.map(|t| format!("{t}; ")).unwrap_or_default();
    Ok(Some(format!("{id}{}: fix: {fix} ({track}pg read {id})", seen.unwrap_or_default())))
}

/// The record id a `known` line is about, when it is in this project.
pub(super) fn known_id(known: &str) -> Option<&str> {
    if known.starts_with("in project ") {
        return None;
    }
    known.split([' ', ':']).next().filter(|id| !id.is_empty())
}

/// The fix another project's memory has for an error line: environment
/// errors (a locked database, line endings, a busy port) repeat across
/// projects.
pub(super) fn known_elsewhere(cx: &Context, here: &crate::project::Project, line: &str) -> Result<Option<String>> {
    for p in cx.home.projects().iter().filter(|p| p.name != here.name) {
        let index = Index::open(p)?;
        if let Some(k) = known(&index, p, line)? {
            return Ok(Some(format!(
                "in project {}: {}",
                p.name,
                k.replace("(pg read ", &format!("(pg read {}:", p.name))
            )));
        }
    }
    Ok(None)
}

/// Files changed (by git) in the project's code folders, as output shows them.
pub(super) fn changed_files(code: &CodeIndex) -> Vec<String> {
    code.folders.iter().flat_map(|f| git::changed(&f.dir).into_iter().map(|(p, _)| code.display(&f.key, &p))).collect()
}

/// The code as it is now, as one hash: each folder's commit and the text
/// of each file changed on it. "" when a folder is not in git (nothing
/// tells two runs' code apart).
pub(super) fn code_state(code: &CodeIndex) -> String {
    let mut parts: Vec<String> = Vec::new();
    for f in &code.folders {
        let Some(head) = git::head(&f.dir) else { return String::new() };
        parts.push(format!("{}@{head}", f.key));
        let mut changed: Vec<String> = git::changed(&f.dir).into_iter().map(|(p, _)| p).collect();
        changed.sort();
        for p in changed {
            let text = std::fs::read(f.dir.join(&p)).unwrap_or_default();
            parts.push(format!("{p}={:x}", crate::code::parse::hash(&String::from_utf8_lossy(&text))));
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    format!(
        "{:x}",
        crate::code::parse::hash(&parts.join(
            "
"
        ))
    )
}

/// The functions changed (by git) now, each with the hash of its body:
/// "payments/refund.go#Refund=9f3a1c2b". Kept with the run, so a later run
/// that passes can tell what changed after this one.
pub(super) fn changed_symbols(code: &CodeIndex) -> Vec<String> {
    crate::code::work::changed(code)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| {
            let s = c.symbol.as_ref()?;
            Some(format!("{}#{}={:x}", code.display(&c.folder, &c.path), s.qualified, s.body_hash))
        })
        .collect()
}

/// The command as one line for the shell, quoting arguments with spaces.
fn join(args: &[String]) -> String {
    if args.len() == 1 {
        return args[0].clone();
    }
    args.iter()
        .map(|a| {
            if a.contains(char::is_whitespace) || a.is_empty() {
                format!("\"{}\"", a.replace('"', "\\\""))
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// cmd /C, passing the line as it is (cmd has its own quoting).
#[cfg(windows)]
fn shell(command: &str) -> std::process::Command {
    let mut c = std::process::Command::new("cmd");
    std::os::windows::process::CommandExt::raw_arg(&mut c, format!("/C {command}"));
    c
}

#[cfg(not(windows))]
fn shell(command: &str) -> std::process::Command {
    use std::os::unix::process::CommandExt;
    let mut c = std::process::Command::new("sh");
    c.args(["-c", command]);
    c.process_group(0); // its own group: a timeout stops everything it started
    c
}

/// Runs a command line in the shell; its exit code and its output (stdout
/// and stderr as they came).
fn execute(
    command: &str,
    cwd: &std::path::Path,
    secrets: &[crate::project::env::Secret],
    timeout: u64,
) -> Result<(i32, String, bool)> {
    let mut c = shell(command);
    let mut child = c
        .current_dir(cwd)
        .envs(secrets.iter().map(|s| (s.name.as_str(), s.value.as_str())))
        // nothing to answer a question with: a command that asks fails
        // instead of waiting forever
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("start the command")?; // its line may hold secrets: not repeated
    let kept = Arc::new(Mutex::new(Kept::default()));
    let readers: Vec<_> = [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|r| {
        let kept = Arc::clone(&kept);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(r);
            let mut buf = Vec::new();
            // lines as bytes: output that is not UTF-8 is read, not dropped
            while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                let line = String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string();
                kept.lock().expect("output lock").push(line);
                buf.clear();
            }
        })
    })
    .collect();
    let deadline = Instant::now() + std::time::Duration::from_secs(timeout.max(1));
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (Some(status), false);
        }
        if Instant::now() >= deadline {
            kill_tree(&mut child);
            break (child.wait().ok(), true);
        }
        // most commands pg runs end within a moment: looked at often then
        let waited = deadline.saturating_duration_since(Instant::now());
        let early = std::time::Duration::from_secs(timeout.max(1)).saturating_sub(waited).as_millis() < 200;
        std::thread::sleep(std::time::Duration::from_millis(if early { 2 } else { 25 }));
    };
    for r in readers {
        let _ = r.join();
    }
    let out = kept.lock().expect("output lock").text();
    let code = if timed_out { TIMED_OUT } else { status.and_then(|s| s.code()).unwrap_or(1) };
    Ok((code, out, timed_out))
}

/// The output kept: the first lines, then the last ones, with how many were
/// left out between.
#[derive(Default)]
struct Kept {
    head: Vec<String>,
    tail: std::collections::VecDeque<String>,
    dropped: usize,
}

impl Kept {
    fn push(&mut self, line: String) {
        let line =
            if line.len() > 2_000 { format!("{}...", line.chars().take(2_000).collect::<String>()) } else { line };
        if self.head.len() < HEAD_LINES {
            self.head.push(line);
            return;
        }
        self.tail.push_back(line);
        if self.tail.len() > TAIL_LINES {
            self.tail.pop_front();
            self.dropped += 1;
        }
    }

    fn text(&self) -> String {
        let mut lines: Vec<String> = self.head.clone();
        if self.dropped > 0 {
            lines.push(format!("... {} lines left out ...", self.dropped));
        }
        lines.extend(self.tail.iter().cloned());
        lines.join("\n")
    }
}

/// Stops the command and every process it started (a test runner's
/// workers would outlive the shell otherwise).
fn kill_tree(child: &mut std::process::Child) {
    let pid = child.id().to_string();
    if cfg!(windows) {
        let _ = std::process::Command::new("taskkill").args(["/T", "/F", "/PID", &pid]).output();
    } else {
        let _ = std::process::Command::new("kill").args(["-KILL", &format!("-{pid}")]).output();
    }
    let _ = child.kill();
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/cli/run.rs"]
mod tests;
