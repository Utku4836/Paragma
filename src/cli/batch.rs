//! pg batch: several pg commands in one call. Those that only look (find,
//! read, tree, ripple, ls, skill, the steps and questions as they are) run
//! side by side; one that changes memory or runs something waits for the
//! ones before it and goes alone, so a later command sees what an earlier
//! one wrote. Each answers on its own, in the order given.
//!
//! ```text
//! pg batch '[["find", "refund window"], ["read", "e12"], ["ripple", "Refund"]]'
//! pg batch < commands.json
//! ```

use super::{Context, USAGE, fail};
use anyhow::Result;
use serde_json::Value;
use std::io::Read;
use std::path::Path;

#[derive(clap::Args)]
pub struct Args {
    /// The commands, as JSON: a list of argument lists (["find", "refund"]); without it, read from stdin
    commands: Option<String>,
}

/// Commands in one batch at most.
pub const MOST: usize = 32;

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let text = match a.commands {
        Some(t) => t,
        None => {
            let mut t = String::new();
            std::io::stdin().read_to_string(&mut t)?;
            t
        }
    };
    let commands = parse(&text).map_err(|e| fail(USAGE, e))?;
    // the batch's project and budget, for a command that names none of its own
    let mut globals = vec!["--budget".to_string(), cx.budget.to_string()];
    if let Some(p) = &cx.project {
        globals.extend(["--project".to_string(), p.name.clone()]);
    }
    let run: Vec<Vec<String>> = commands.iter().map(|c| with_globals(c.clone(), &globals)).collect();
    let done = run_all(&cx.cwd, &run);
    // each named as it was given
    print!("{}", report(&commands, &done));
    // the worst of them: 1 (nothing found) is an answer, more a failure
    let worst = done.iter().map(|(c, _)| *c).max().unwrap_or(0);
    if worst > 1 {
        return Err(fail(7, "a command of the batch failed (above)"));
    }
    Ok(())
}

/// The batch's commands from its JSON: a list of argument lists, or of
/// objects with "args".
pub fn parse(text: &str) -> Result<Vec<Vec<String>>, String> {
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| {
        format!(
            "give the commands as JSON, a list of argument lists: [[\"find\", \"refund\"], [\"read\", \"e12\"]] ({e})"
        )
    })?;
    let Some(items) = v.as_array() else { return Err("the commands go in a list: [[\"find\", \"refund\"]]".into()) };
    if items.is_empty() {
        return Err("no commands in the list".into());
    }
    if items.len() > MOST {
        return Err(format!("{} commands; at most {MOST} in one batch", items.len()));
    }
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let list = item.get("args").unwrap_or(item);
        let args: Option<Vec<String>> = list.as_array().map(|a| {
            a.iter()
                .filter_map(|x| match x {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    Value::Bool(b) => Some(b.to_string()),
                    _ => None,
                })
                .collect()
        });
        let Some(mut args) = args.filter(|a| !a.is_empty()) else {
            return Err(format!("command {}: give it as a list of arguments, [\"find\", \"refund\"]", i + 1));
        };
        if args[0] == "pg" {
            args.remove(0);
        }
        match args.first().map(String::as_str) {
            None => return Err(format!("command {}: empty", i + 1)),
            Some("batch" | "mcp" | "hook" | "sleep") => {
                return Err(format!("command {}: pg {} cannot run inside a batch", i + 1, args[0]));
            }
            _ => {}
        }
        out.push(args);
    }
    Ok(out)
}

/// The global flags the batch was given, for a command that has none of its own.
fn with_globals(mut c: Vec<String>, globals: &[String]) -> Vec<String> {
    // what follows `--` is the command's own (pg run -- tool --budget 3)
    let own = c.iter().position(|a| a == "--").unwrap_or(c.len());
    let has = |flag: &str| c[..own].iter().any(|a| a == flag || a.starts_with(&format!("{flag}=")));
    let mut add = Vec::new();
    for pair in globals.chunks(2) {
        let own = has(&pair[0]) || (pair[0] == "--project" && has("-p"));
        if !own {
            add.extend(pair.iter().cloned());
        }
    }
    // after the subcommand's own arguments a `--` may come (pg run -- cargo
    // test): the flags go first
    add.append(&mut c);
    add
}

/// Whether a command only looks: it may run beside others.
pub fn only_looks(args: &[String]) -> bool {
    let rest = &args[1.min(args.len())..];
    match args.first().map(String::as_str) {
        Some("find" | "read" | "tree" | "ripple" | "ls" | "skill") => true,
        // the plan and the questions as they are
        Some("steps" | "ask") => only(rest, &["--all"]),
        _ => false,
    }
}

/// Whether the arguments are only these flags and the global ones.
fn only(rest: &[String], allowed: &[&str]) -> bool {
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if ["-p", "--project", "--budget"].contains(&a.as_str()) {
            it.next(); // its value
        } else if !(a.starts_with("--project=") || a.starts_with("--budget=") || allowed.contains(&a.as_str())) {
            return false;
        }
    }
    true
}

/// Runs the commands: those that only look side by side, the others one at
/// a time in their place. Each one's exit code and output, in order.
pub fn run_all(cwd: &Path, commands: &[Vec<String>]) -> Vec<(i32, String)> {
    let mut out: Vec<Option<(i32, String)>> = vec![None; commands.len()];
    let mut i = 0;
    while i < commands.len() {
        if !only_looks(&commands[i]) {
            out[i] = Some(pg(cwd, &commands[i]));
            i += 1;
            continue;
        }
        let start = i;
        while i < commands.len() && only_looks(&commands[i]) {
            i += 1;
        }
        let group = &commands[start..i];
        let results: Vec<(i32, String)> = std::thread::scope(|s| {
            let handles: Vec<_> = group.iter().map(|c| s.spawn(move || pg(cwd, c))).collect();
            handles.into_iter().map(|h| h.join().unwrap_or((7, "pg stopped".into()))).collect()
        });
        for (k, r) in results.into_iter().enumerate() {
            out[start + k] = Some(r);
        }
    }
    out.into_iter().map(|r| r.unwrap_or((7, String::new()))).collect()
}

/// Each command's answer under a line naming it.
pub fn report(commands: &[Vec<String>], done: &[(i32, String)]) -> String {
    let mut s = String::new();
    for (k, (c, (code, text))) in commands.iter().zip(done).enumerate() {
        let shown: Vec<String> = c.iter().map(|a| if a.contains(' ') { format!("{a:?}") } else { a.clone() }).collect();
        let status = match code {
            0 => String::new(),
            1 => " (nothing found)".to_string(),
            n => format!(" (failed, exit {n})"),
        };
        if k > 0 {
            s.push('\n');
        }
        s.push_str(&format!("── {}. pg {}{status}\n", k + 1, crate::output::one_line(&shown.join(" "), 100)));
        if !text.is_empty() {
            s.push_str(text);
            s.push('\n');
        }
    }
    s
}

/// Runs pg itself with these arguments in `cwd`: its exit code and what it
/// printed.
pub fn pg(cwd: &Path, argv: &[String]) -> (i32, String) {
    let Ok(exe) = std::env::current_exe() else { return (7, "pg not found".into()) };
    match std::process::Command::new(exe).args(argv).current_dir(cwd).stdin(std::process::Stdio::null()).output() {
        Ok(o) => (
            o.status.code().unwrap_or(7),
            format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
                .trim_end()
                .to_string(),
        ),
        Err(e) => (7, format!("pg could not start: {e}")),
    }
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/cli/batch.rs"]
mod tests;
