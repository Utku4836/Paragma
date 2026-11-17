//! Reading a test or build run's output: which tests failed and passed,
//! and the lines that say what went wrong (with the file and line they
//! point at). Knows the output of go test, cargo test, pytest, jest and
//! vitest, dotnet test; any other output still gives its error lines.

#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub failed: Vec<String>,
    pub passed: usize,
    /// The lines that say what went wrong, most telling first.
    pub errors: Vec<String>,
}

/// Error lines kept.
const ERRORS: usize = 5;

/// Whether the output says a test suite passed without listing its tests:
/// go test's "ok  shop/payments 0.01s", cargo's "test result: ok.",
/// pytest's "5 passed in 0.1s", jest's "Tests: 5 passed", dotnet's
/// "Passed!".
pub fn suite_passed(output: &str) -> bool {
    output.lines().map(str::trim).any(|line| {
        let lower = line.to_lowercase();
        let words: Vec<&str> = line.split_whitespace().collect();
        let go = words.len() >= 3
            && words[0] == "ok"
            && words[1].contains(|c: char| c.is_alphanumeric())
            && words[2..]
                .iter()
                .any(|w| *w == "(cached)" || w.strip_suffix('s').is_some_and(|n| n.parse::<f64>().is_ok()));
        let counted = lower.contains(" passed") && !lower.contains("failed") && {
            let at = lower.find(" passed").unwrap_or(0);
            lower[..at].rsplit(|c: char| !c.is_ascii_digit()).next().is_some_and(|n| !n.is_empty())
        };
        go || lower.starts_with("test result: ok.") || lower.starts_with("passed!") || counted
    })
}

/// Whether the output has a test or build tool's own word that it failed:
/// go's "FAIL\tshop/payments" or "[build failed]", cargo's "test result:
/// FAILED" or "could not compile", pytest's "1 failed", jest's "Tests: 1
/// failed", dotnet's "Failed!" or "Build FAILED". Error-like lines alone
/// (a source file printed, a log) are not.
pub fn suite_failed(output: &str) -> bool {
    output.lines().map(str::trim).any(|line| {
        let lower = line.to_lowercase();
        let counted = lower.contains(" failed") && {
            let at = lower.find(" failed").unwrap_or(0);
            lower[..at].rsplit(|c: char| !c.is_ascii_digit()).next().is_some_and(|n| !n.is_empty() && n != "0")
        };
        line == "FAIL"
            || line.starts_with("FAIL\t")
            || line.starts_with("--- FAIL")
            || lower.contains("[build failed]")
            || lower.starts_with("test result: failed")
            || lower.starts_with("error: could not compile")
            || lower.starts_with("failed!")
            || lower.starts_with("build failed")
            || (counted && (lower.contains(" passed") || lower.contains(" in ") || lower.starts_with("tests:")))
    })
}

pub fn parse(output: &str) -> Parsed {
    let mut p = Parsed::default();
    let mut with_place = Vec::new();
    let mut without = Vec::new();
    let all: Vec<&str> = output.lines().collect();
    for (k, raw) in all.iter().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        match test_line(line) {
            Some((name, false)) => {
                if !p.failed.contains(&name) {
                    p.failed.push(name);
                }
                continue;
            }
            Some((_, true)) => {
                p.passed += 1;
                continue;
            }
            None => {}
        }
        // "file:line: message" is how compilers and test frameworks report
        let at_place = locations(line).first().is_some_and(|(f, _)| line.starts_with(f.as_str()));
        if is_error(line) || at_place {
            // rustc and tsc say where on the next line: "--> src/a.rs:10:5"
            let mut line = line.to_string();
            if locations(&line).is_empty()
                && let Some(at) = all.get(k + 1).map(|n| n.trim()).and_then(|n| n.strip_prefix("--> "))
                && !locations(at).is_empty()
            {
                line = format!("{line} --> {at}");
            }
            let list = if locations(&line).is_empty() { &mut without } else { &mut with_place };
            let l: String = line.chars().take(300).collect();
            if !list.contains(&l) {
                list.push(l);
            }
        }
    }
    p.errors = with_place.into_iter().chain(without).take(ERRORS).collect();
    p
}

/// A test result line: (test name, passed).
fn test_line(line: &str) -> Option<(String, bool)> {
    let first = |s: &str| s.split_whitespace().next().unwrap_or("").to_string();
    // go test
    if let Some(r) = line.strip_prefix("--- FAIL: ") {
        return Some((first(r), false));
    }
    if let Some(r) = line.strip_prefix("--- PASS: ") {
        return Some((first(r), true));
    }
    // cargo test
    if let Some(r) = line.strip_prefix("test ") {
        if let Some(name) = r.strip_suffix(" ... FAILED") {
            return Some((name.to_string(), false));
        }
        if let Some(name) = r.strip_suffix(" ... ok") {
            return Some((name.to_string(), true));
        }
    }
    // pytest
    if let Some(r) = line.strip_prefix("FAILED ") {
        return Some((first(r.split(" - ").next().unwrap_or(r)), false));
    }
    if line.contains("::") && line.ends_with(" PASSED") || line.contains("::") && line.contains(" PASSED [") {
        return Some((first(line), true));
    }
    // jest, vitest
    for mark in ["✕ ", "× ", "✗ "] {
        if let Some(r) = line.strip_prefix(mark) {
            return Some((without_time(r), false));
        }
    }
    for mark in ["✓ ", "√ ", "✔ "] {
        if let Some(r) = line.strip_prefix(mark) {
            return Some((without_time(r), true));
        }
    }
    // dotnet test
    if let Some(r) = line.strip_prefix("Failed ") {
        return Some((first(r), false));
    }
    if let Some(r) = line.strip_prefix("Passed ") {
        return Some((first(r), true));
    }
    None
}

/// "adds two numbers (5 ms)" -> "adds two numbers".
fn without_time(s: &str) -> String {
    match s.rsplit_once(" (") {
        Some((name, t)) if t.ends_with("ms)") || t.ends_with(" s)") => name.trim().to_string(),
        _ => s.trim().to_string(),
    }
}

fn is_error(line: &str) -> bool {
    let lower = line.to_lowercase();
    const MARKS: &[&str] = &[
        "error",
        "panic",
        "fail",
        "exception",
        "traceback",
        "assert",
        "expected",
        "mismatch",
        "undefined",
        "not found",
        "cannot",
        "denied",
        "timeout",
        "timed out",
    ];
    // summary lines say nothing about the failure itself
    let summary = lower == "fail"
        || lower.starts_with("fail\t")
        || lower.starts_with("ok ")
        || lower.starts_with("test result:")
        || lower.starts_with("error: test failed")
        || lower.starts_with("failures:")
        || lower.starts_with("=====")
        || lower.starts_with("tests:");
    !summary && MARKS.iter().any(|m| lower.contains(m))
}

/// The file:line places a line points at ("payments/refund_test.go:41").
pub fn locations(line: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    // Python: File "shop/cart.py", line 12, in total
    if let Some(rest) = line.trim_start().strip_prefix("File \"")
        && let Some((file, after)) = rest.split_once('"')
        && let Some(n) = after.strip_prefix(", line ").and_then(|a| a.split(|c: char| !c.is_ascii_digit()).next())
        && let Ok(n) = n.parse::<usize>()
    {
        out.push((file.to_string(), n));
        return out;
    }
    for token in line.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '"' | '\'' | ',' | '[' | ']')) {
        let parts: Vec<&str> = token.split(':').collect();
        for i in 0..parts.len().saturating_sub(1) {
            let file = parts[..=i].join(":");
            let has_ext = file.rsplit(['/', '\\']).next().is_some_and(|n| n.contains('.') && !n.ends_with('.'));
            if let Ok(n) = parts[i + 1].parse::<usize>()
                && has_ext
                && n > 0
            {
                out.push((file.trim_start_matches("./").to_string(), n));
                break;
            }
        }
    }
    out
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/testrun/parse.rs"]
mod tests;
