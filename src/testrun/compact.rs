//! A command's output made short for an agent, keeping what it says:
//! errors whole (with the location line compilers put after them) wherever
//! they are, warnings grouped by what they say, the same line repeated
//! said once with how many times, progress lines (Compiling, Downloading,
//! percentages) counted, not shown, and the end of the output, where tools
//! put their summary.

use regex::Regex;
use std::sync::LazyLock;

/// Lines of one error kept (its first line, where, and what explains it).
const ERROR_LINES: usize = 8;
/// Errors kept whole; the others by their first line.
const ERRORS_WHOLE: usize = 6;
/// Places listed for a warning seen in many.
const PLACES: usize = 4;

/// A line that only reports progress.
static PROGRESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)^\s*(
            (compiling|checking|downloaded|downloading|fetching|fetched|updating|installing|resolving|unpacking|building|linking|locking|adding|preparing|collecting|requirement\ already\ satisfied|using\ cached|blocking\ waiting|fresh)\b
            | \[?[=\#>\-\.\s]{8,}\]?\s*\d*%?
            | \d{1,3}(\.\d+)?\s?%(\s|$)
            | (npm\ )?(http\ )?fetch\ (get|post)\b
            | added\ \d+\ packages?
            | [⠁-⣿|/\\-]\s
        )",
    )
    .expect("progress pattern")
});

/// The first line of a diagnostic: an error or a warning.
static DIAGNOSTIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(error|warning|warn|fatal|panic|exception|traceback|caused by)\b|^\S+:\d+(:\d+)?:\s*(error|warning)\b|\berror(\[\w+\])?:|^thread '.*' panicked")
        .expect("diagnostic pattern")
});

static WARNING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*(warning|warn)\b|:\s*warning\b").expect("warning pattern"));

/// Where a diagnostic is: "--> src/a.rs:10:5", "at foo (src/a.ts:3:1)",
/// "File \"a.py\", line 3", "a.go:10:".
static PLACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:-->\s*|\bat\s.*\(|File ")?([\w./\\-]+\.[A-Za-z]{1,5})(?:"?, line |:)(\d+)"#)
        .expect("place pattern")
});

/// A line of code a compiler quotes: "10 |     net * rate".
static EXCERPT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\d+\s*\|").expect("excerpt pattern"));

/// The ANSI colour codes of a terminal.
static ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").expect("ansi pattern"));

/// The output as an agent should read it, in at most `max` lines: errors
/// first (in the order they came), then grouped warnings, then the end of
/// the output; what was left out is counted.
pub fn compact(output: &str, max: usize) -> Vec<String> {
    let lines = clean(output);
    let mut errors: Vec<Vec<String>> = Vec::new();
    // warning headline -> (count, places, first block)
    let mut warnings: Vec<(String, usize, Vec<String>, Vec<String>)> = Vec::new();
    let mut progress = 0usize;
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = &lines[i];
        if PROGRESS.is_match(l) {
            progress += 1;
            i += 1;
            continue;
        }
        if DIAGNOSTIC.is_match(l) {
            // the block: its first line and what follows it, indented or
            // pointing at code, up to a blank line or the next diagnostic
            let mut block = vec![l.clone()];
            let mut j = i + 1;
            while j < lines.len() && !lines[j].trim().is_empty() && !DIAGNOSTIC.is_match(&lines[j]) {
                let t = &lines[j];
                if t.starts_with(char::is_whitespace)
                    || t.trim_start().starts_with(['|', '=', '-', '^', '>'])
                    || EXCERPT.is_match(t)
                {
                    block.push(t.clone());
                    j += 1;
                } else {
                    break;
                }
            }
            i = j;
            if WARNING.is_match(l) {
                let head = normalize(l);
                let place = block.iter().find_map(|b| place_of(b));
                match warnings.iter_mut().find(|w| w.0 == head) {
                    Some(w) => {
                        w.1 += 1;
                        if let Some(p) = place
                            && !w.2.contains(&p)
                        {
                            w.2.push(p);
                        }
                    }
                    None => warnings.push((head, 1, place.into_iter().collect(), block)),
                }
            } else if !errors.iter().any(|e| e[0] == block[0]) {
                errors.push(block);
            }
            continue;
        }
        rest.push(l.clone());
        i += 1;
    }

    let mut out: Vec<String> = Vec::new();
    for (n, e) in errors.iter().enumerate() {
        if n < ERRORS_WHOLE {
            out.extend(trim_block(e));
        } else {
            out.push(e[0].clone());
        }
    }
    for (_, count, places, block) in &warnings {
        let first = block[0].trim().to_string();
        if *count == 1 {
            out.extend(trim_block(block).into_iter().take(3));
            continue;
        }
        let shown: Vec<&str> = places.iter().take(PLACES).map(String::as_str).collect();
        let more = places.len().saturating_sub(PLACES);
        out.push(format!(
            "{first} (×{count}{}{})",
            if shown.is_empty() { String::new() } else { format!(": {}", shown.join(", ")) },
            if more > 0 { format!(" +{more}") } else { String::new() }
        ));
    }
    // the end of the output, the same line said once
    let tail = merge_repeats(&rest);
    let room = max.saturating_sub(out.len() + 1).max(max.min(8));
    let from = tail.len().saturating_sub(room);
    if from > 0 {
        out.push(format!("... {} lines before ...", from));
    }
    out.extend(tail[from..].iter().cloned());
    if progress > 0 {
        out.push(format!("({progress} progress lines left out)"));
    }
    if out.len() > max {
        let cut = out.len() - max;
        out.drain(max.saturating_sub(1)..out.len() - 1);
        let last = out.pop().unwrap_or_default();
        out.push(format!("... {cut} more lines ..."));
        out.push(last);
    }
    out
}

/// Lines without colour codes, a progress bar's line as it last read
/// (after its last carriage return), trailing spaces and blank runs.
fn clean(output: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in output.lines() {
        let l = ANSI.replace_all(raw, "");
        let l = l.rsplit('\r').next().unwrap_or("").trim_end().to_string();
        if l.trim().is_empty() && out.last().is_some_and(|p| p.trim().is_empty()) {
            continue;
        }
        out.push(l);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out
}

/// A block's first lines that matter: its first line, where it is, and
/// what explains it (not the whole code excerpt).
fn trim_block(block: &[String]) -> Vec<String> {
    if block.len() <= ERROR_LINES {
        return block.to_vec();
    }
    let mut out = vec![block[0].clone()];
    for l in &block[1..] {
        let t = l.trim_start();
        if out.len() >= ERROR_LINES - 1 {
            break;
        }
        if place_of(l).is_some()
            || t.starts_with('=')
            || t.starts_with("help")
            || t.starts_with("note")
            || out.len() < 4
        {
            out.push(l.clone());
        }
    }
    out.push(format!("  ... ({} more lines)", block.len() - out.len()));
    out
}

/// "src/a.rs:10" from a line that says where.
fn place_of(line: &str) -> Option<String> {
    PLACE.captures(line).map(|c| format!("{}:{}", c[1].replace('\\', "/"), &c[2]))
}

/// A line without what changes between repeats of it: numbers, quoted
/// names, places.
fn normalize(line: &str) -> String {
    static VAR: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"`[^`]*`|'[^']*'|\x22[^\x22]*\x22|\d+").expect("normalize pattern"));
    VAR.replace_all(line.trim(), "_").to_string()
}

/// Repeated lines (the same but for numbers) said once, with how many.
fn merge_repeats(lines: &[String]) -> Vec<String> {
    let mut out: Vec<(String, String, usize)> = Vec::new();
    for l in lines {
        let n = normalize(l);
        match out.last_mut() {
            Some(last) if last.1 == n && !n.is_empty() && n != "_" => last.2 += 1,
            _ => out.push((l.clone(), n, 1)),
        }
    }
    out.into_iter().map(|(l, _, c)| if c > 1 { format!("{l} (×{c})") } else { l }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rust_build_with_noise_errors_and_warnings() {
        let mut log = String::new();
        for i in 0..120 {
            log.push_str(&format!("   Compiling crate{i} v0.{i}.0\n"));
        }
        for f in ["src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs", "src/e.rs", "src/f.rs"] {
            log.push_str(&format!(
                "warning: unused variable: `x`\n --> {f}:3:9\n  |\n3 |     let x = 1;\n  |         ^ help: prefix it with an underscore: `_x`\n\n"
            ));
        }
        log.push_str("error[E0425]: cannot find value `rate` in this scope\n --> src/tax.rs:10:20\n   |\n10 |     net * rate\n   |           ^^^^ not found in this scope\n\n");
        log.push_str("error: could not compile `shop` (bin \"shop\") due to 1 previous error; 6 warnings emitted\n");
        let out = compact(&log, 30);
        let text = out.join("\n");
        assert!(text.contains("error[E0425]: cannot find value `rate` in this scope"), "{text}");
        assert!(text.contains("--> src/tax.rs:10:20"), "{text}");
        assert!(
            text.contains("warning: unused variable: `x` (×6: src/a.rs:3, src/b.rs:3, src/c.rs:3, src/d.rs:3 +2)"),
            "{text}"
        );
        assert!(text.contains("(120 progress lines left out)"), "{text}");
        assert!(!text.contains("Compiling crate"), "{text}");
        assert!(out.len() < 20, "{text}");
    }

    #[test]
    fn an_error_in_the_middle_is_kept_and_repeats_merged() {
        let mut log = String::from("starting\n");
        for i in 0..50 {
            log.push_str(&format!("GET /health 200 {i}ms\n"));
        }
        log.push_str(
            "Error: connect ECONNREFUSED 127.0.0.1:5432\n    at TCPConnectWrap.afterConnect (node:net:1555:16)\n",
        );
        for i in 0..50 {
            log.push_str(&format!("GET /health 200 {i}ms\n"));
        }
        log.push_str("done\n");
        let out = compact(&log, 12);
        let text = out.join("\n");
        assert!(text.starts_with("Error: connect ECONNREFUSED 127.0.0.1:5432"), "{text}");
        // the error is said first, so the lines around it are one run
        assert!(text.contains("GET /health 200 0ms (×100)"), "{text}");
        assert!(text.ends_with("done"), "{text}");
        assert!(out.len() <= 12, "{text}");
    }

    #[test]
    fn colours_and_progress_bars_are_cleaned() {
        let log = "\x1b[32mok\x1b[0m\n[=====>        ] 40%\r[==============] 100%\nadded 312 packages in 4s\nready\n";
        let out = compact(log, 10);
        assert_eq!(out, vec!["ok", "ready", "(2 progress lines left out)"]);
    }
}
