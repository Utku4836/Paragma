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

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let command = join(&a.command);
    let started = Instant::now();
    let secrets = cx.project.as_ref().map(crate::project::env::load).unwrap_or_default();
    let line = crate::project::env::substitute(&command, &secrets);
    let (exit, output, timed_out) = execute(&line, &cx.cwd, &secrets, a.timeout)?;
    let output = crate::project::env::hide(&output, &secrets);
    // as shown and kept in the history of runs: without any value typed in it
    let command = crate::project::env::hide(&command, &secrets);
    let parsed = parse::parse(&output);
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
        passed: parsed.passed,
        errors: parsed.errors.clone(),
        changed: code.as_ref().map(changed_files).unwrap_or_default(),
    };

    let mut b = Budget::new(cx.budget);
    let tests = if run.failing.is_empty() && run.passed == 0 {
        String::new()
    } else if run.failing.is_empty() {
        format!(": {} passed", run.passed)
    } else {
        format!(": {} failing, {} passed", run.failing.len(), run.passed)
    };
    if timed_out {
        b.add(&format!("fail {command}: stopped after {} s (--timeout), with what it started{tests}", a.timeout));
    } else if exit == 0 {
        b.add(&format!("ok {command} ({:.1} s){tests}", run.seconds));
    } else {
        b.add(&format!("fail {command} (exit {exit}, {:.1} s){tests}", run.seconds));
    }

    let Some(project) = &cx.project else {
        tail(&mut b, &output, if exit == 0 { 3 } else { 40 });
        b.add("(no project here: pg open keeps the history of runs and finds known fixes)");
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
    }

    let index = Index::open(project)?;
    let mut unexplained: Option<(String, Vec<String>)> = None;
    if exit != 0 && !run.errors.is_empty() {
        b.add("errors:");
        for line in run.errors.iter().take(EXPLAINED) {
            b.add(line);
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
            let known = match known(&index, project, line)? {
                Some(k) => Some(k),
                None => known_elsewhere(cx, project, line)?,
            };
            match known {
                Some(k) => {
                    b.add(&format!("  known: {k}"));
                }
                None if unexplained.is_none() => unexplained = Some((line.clone(), files_hit)),
                None => {}
            }
        }
    }
    let tests_seen = run.passed > 0 || !run.failing.is_empty();
    if exit != 0 && run.errors.is_empty() {
        tail(&mut b, &output, 30); // nothing understood: the output says it best
    } else if exit == 0 && !tests_seen {
        tail(&mut b, &output, 60); // not a test run (an API call, a script): its output is the answer
    }
    if let Some((line, files)) = unexplained {
        let files = if files.is_empty() { String::new() } else { format!(" --files {}", files.join(",")) };
        b.add(&format!(
            "once fixed, remember it: pg write --error {:?} --cause \"...\" --fix \"...\"{files}",
            one_line(&line, 120)
        ));
    } else if !verdict.fixed.is_empty()
        && let Some(line) = history.first().and_then(|r| r.errors.first())
        && known(&index, project, line)?.is_none()
    {
        b.add(&format!("worth remembering? pg write --error {:?} --cause \"...\" --fix \"...\"", one_line(line, 120)));
        state.event("fixed", line, &verdict.fixed.join(", "))?;
    }

    state.record_run(&run)?;
    let detail = if exit == 0 { "pass".to_string() } else { format!("fail {}", run.failing.len()) };
    state.event("run", &command, &detail)?;
    finish(&b, exit);
}

/// Prints and ends with the command's exit code.
fn finish(b: &Budget, exit: i32) -> ! {
    print!("{}", b.out);
    let _ = std::io::stdout().flush();
    std::process::exit(if exit == 0 { 0 } else { exit.clamp(1, 255) });
}

/// The last lines of the output that fit.
fn tail(b: &mut Budget, output: &str, lines: usize) {
    let all: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();
    let from = all.len().saturating_sub(lines);
    let mut shown: Vec<&str> = all[from..].to_vec();
    while !shown.is_empty() && !b.add(&format!("output:\n{}", shown.join("\n"))) {
        shown.remove(0);
    }
}

/// The note memory has about an error line, with its fix.
pub(super) fn known(index: &Index, project: &crate::project::Project, line: &str) -> Result<Option<String>> {
    let Some(path) = record::known_error(index, line)? else { return Ok(None) };
    let note = Note::parse(&files::read_text(&project.dir.join(&path))?);
    let id = note.id().unwrap_or("?").to_string();
    let seen =
        note.props.text("seen").and_then(|s| s.parse::<i64>().ok()).map(|n| format!(" (seen {})", count(n, "time")));
    // the fix is what the agent needs: in full, up to a short paragraph
    let fix = note.section("Fix").map(|f| one_line(&f, 300)).unwrap_or_else(|| one_line(&note.body, 300));
    Ok(Some(format!("{id}{}: fix: {fix} (pg read {id})", seen.unwrap_or_default())))
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
        std::thread::sleep(std::time::Duration::from_millis(25));
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

#[cfg(test)]
mod tests {
    #[test]
    fn commands_are_joined_for_the_shell() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(super::join(&args(&["go", "test", "./..."])), "go test ./...");
        assert_eq!(super::join(&args(&["pytest", "-k", "refund window"])), "pytest -k \"refund window\"");
        assert_eq!(super::join(&args(&["npm test && echo ok"])), "npm test && echo ok");
    }
}
