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
    /// The command, after --: pg run -- go test ./...
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true, value_name = "COMMAND")]
    command: Vec<String>,
}

/// Earlier runs of a command compared with.
const HISTORY: usize = 10;
/// Error lines explained.
const EXPLAINED: usize = 3;

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let command = join(&a.command);
    let started = Instant::now();
    let secrets = cx.project.as_ref().map(crate::project::env::load).unwrap_or_default();
    let line = crate::project::env::substitute(&command, &secrets);
    let (exit, output) = execute(&line, &cx.cwd, &secrets)?;
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
    if exit == 0 {
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
            match known(&index, project, line)? {
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
fn known(index: &Index, project: &crate::project::Project, line: &str) -> Result<Option<String>> {
    let Some(path) = record::known_error(index, line)? else { return Ok(None) };
    let note = Note::parse(&files::read_text(&project.dir.join(&path))?);
    let id = note.id().unwrap_or("?").to_string();
    let seen =
        note.props.text("seen").and_then(|s| s.parse::<i64>().ok()).map(|n| format!(" (seen {})", count(n, "time")));
    let fix = note.section("Fix").map(|f| one_line(&f, 100)).unwrap_or_else(|| one_line(&note.body, 100));
    Ok(Some(format!("{id}{}: fix: {fix} (pg read {id})", seen.unwrap_or_default())))
}

/// Files changed (by git) in the project's code folders, as output shows them.
fn changed_files(code: &CodeIndex) -> Vec<String> {
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
    let mut c = std::process::Command::new("sh");
    c.args(["-c", command]);
    c
}

/// Runs a command line in the shell; its exit code and its output (stdout
/// and stderr as they came).
fn execute(command: &str, cwd: &std::path::Path, secrets: &[crate::project::env::Secret]) -> Result<(i32, String)> {
    let mut c = shell(command);
    let mut child = c
        .current_dir(cwd)
        .envs(secrets.iter().map(|s| (s.name.as_str(), s.value.as_str())))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("start the command")?; // its line may hold secrets: not repeated
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let readers: Vec<_> = [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|r| {
        let lines = Arc::clone(&lines);
        std::thread::spawn(move || {
            for l in BufReader::new(r).lines().map_while(Result::ok) {
                lines.lock().expect("output lock").push(l);
            }
        })
    })
    .collect();
    let status = child.wait()?;
    for r in readers {
        let _ = r.join();
    }
    let out = lines.lock().expect("output lock").join("\n");
    Ok((status.code().unwrap_or(1), out))
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
