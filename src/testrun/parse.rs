//! Reading a test or build run's output: which tests failed and passed,
//! and the lines that say what went wrong (with the file and line they
//! point at). Knows the output of go test, cargo test, pytest, jest and
//! vitest, node --test (spec and TAP), mocha, dotnet test; any other output
//! still gives its error lines. The numbers of tests are the tool's own when
//! it says them (`counts`); the lines give the names.

#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub failed: Vec<String>,
    pub passed: usize,
    /// The lines that say what went wrong, most telling first.
    pub errors: Vec<String>,
}

/// Error lines kept.
const ERRORS: usize = 5;

/// The numbers of tests as the test tool itself said them in its summary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Counts {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    /// The tool that said them: "node", "cargo", "pytest", ...
    pub tool: &'static str,
}

/// A line without the ANSI colour codes of a terminal.
fn plain(line: &str) -> std::borrow::Cow<'_, str> {
    static ANSI: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").expect("ansi pattern"));
    ANSI.replace_all(line, "")
}

/// "5 passed" (or "Failed: 1", "tests 12") in a list of such: (5, "passed").
fn number_word(item: &str) -> Option<(usize, String)> {
    let mut w = item.split_whitespace();
    let (a, b) = (w.next()?, w.next()?);
    if let Ok(n) = a.parse::<usize>() {
        return Some((n, b.trim_end_matches([',', ';', '.']).to_lowercase()));
    }
    let n = b.trim_end_matches([',', ';']).parse::<usize>().ok()?;
    Some((n, a.trim_end_matches(':').to_lowercase()))
}

/// Counts from (number, word) items: passed, failed (errors, cancelled
/// too), skipped (ignored, pending, todo too).
fn tally(items: &[(usize, String)], tool: &'static str) -> Counts {
    let mut c = Counts { tool, ..Default::default() };
    for (n, w) in items {
        match w.as_str() {
            "passed" | "passing" | "pass" | "xpassed" => c.passed += n,
            "failed" | "failing" | "fail" | "error" | "errors" | "cancelled" => c.failed += n,
            "skipped" | "ignored" | "pending" | "todo" | "xfailed" | "skip" => c.skipped += n,
            _ => {}
        }
    }
    c
}

fn add(acc: &mut Option<Counts>, c: Counts) {
    let a = acc.get_or_insert(Counts { tool: c.tool, ..Default::default() });
    a.passed += c.passed;
    a.failed += c.failed;
    a.skipped += c.skipped;
}

/// The numbers of tests from the test tool's own summary, when it says
/// them: node --test (spec "ℹ tests 12", TAP "# tests 12"), cargo ("test
/// result: ok. 5 passed; 1 failed; 2 ignored", added up over its test
/// binaries), pytest ("1 failed, 5 passed, 2 skipped in 0.12s"), jest
/// ("Tests: 1 failed, 5 passed, 6 total"), vitest ("Tests  1 failed | 5
/// passed (6)"), mocha ("5 passing", "1 failing", "2 pending"), dotnet
/// ("Failed!  - Failed: 1, Passed: 5, Skipped: 0, Total: 6").
pub fn counts(output: &str) -> Option<Counts> {
    let mut node: Vec<(String, usize)> = Vec::new();
    let (mut cargo, mut dotnet, mut mocha, mut pytest) = (None, None, None, None);
    let mut jest: Option<Counts> = None;
    for raw in output.lines() {
        let line = plain(raw);
        let line = line.trim();
        // node: one number a line, in a block at the end
        if let Some(r) = line.strip_prefix("ℹ ").or_else(|| line.strip_prefix("# ")) {
            let mut w = r.split_whitespace();
            if let (Some(k), Some(v), None) = (w.next(), w.next(), w.next())
                && let Ok(n) = v.parse::<usize>()
                && matches!(k, "tests" | "suites" | "pass" | "fail" | "cancelled" | "skipped" | "todo")
            {
                node.push((k.to_string(), n));
            }
            continue;
        }
        let lower = line.to_lowercase();
        if let Some(r) = lower.strip_prefix("test result: ") {
            let items: Vec<(usize, String)> = r.split([';', '.']).filter_map(|i| number_word(i.trim())).collect();
            add(&mut cargo, tally(&items, "cargo"));
            continue;
        }
        if (lower.starts_with("failed!") || lower.starts_with("passed!")) && line.contains(" - ") {
            let r = line.split_once(" - ").map(|p| p.1).unwrap_or("");
            let items: Vec<(usize, String)> = r.split(',').filter_map(|i| number_word(i.trim())).collect();
            add(&mut dotnet, tally(&items, "dotnet"));
            continue;
        }
        if let Some((n, w)) = number_word(line)
            && line.split_whitespace().count() <= 3
            && matches!(w.as_str(), "passing" | "failing" | "pending")
            && line.split_whitespace().next().is_some_and(|f| f.parse::<usize>().is_ok())
        {
            add(&mut mocha, tally(&[(n, w)], "mocha"));
            continue;
        }
        // jest "Tests:       1 failed, 5 passed, 6 total"; vitest "Tests  1 failed | 5 passed (6)"
        if let Some((r, tool)) = line
            .strip_prefix("Tests:")
            .map(|r| (r, "jest"))
            .or_else(|| line.strip_prefix("Tests ").map(|r| (r, "vitest")))
        {
            let r = r.split(" (").next().unwrap_or(r);
            let items: Vec<(usize, String)> = r.split([',', '|']).filter_map(|i| number_word(i.trim())).collect();
            if !items.is_empty() {
                jest = Some(tally(&items, tool));
            }
            continue;
        }
        // pytest "==== 1 failed, 5 passed in 0.12s ====", "5 passed in 0.1s"
        let bare = line.trim_matches(|c| c == '=' || c == ' ');
        if let Some((list, time)) = bare.rsplit_once(" in ")
            && time.trim_start().starts_with(|c: char| c.is_ascii_digit())
        {
            let items: Vec<(usize, String)> = list.split(',').filter_map(|i| number_word(i.trim())).collect();
            let all_known = items.iter().all(|(_, w)| {
                matches!(
                    w.as_str(),
                    "passed"
                        | "failed"
                        | "skipped"
                        | "error"
                        | "errors"
                        | "xfailed"
                        | "xpassed"
                        | "deselected"
                        | "warning"
                        | "warnings"
                        | "rerun"
                )
            });
            if !items.is_empty() && all_known {
                pytest = Some(tally(&items, "pytest"));
            }
        }
    }
    if node.iter().any(|(k, _)| k == "tests") && node.iter().any(|(k, _)| k == "pass") {
        let get = |k: &str| node.iter().rev().find(|(x, _)| x == k).map(|(_, n)| *n).unwrap_or(0);
        return Some(Counts {
            passed: get("pass"),
            failed: get("fail") + get("cancelled"),
            skipped: get("skipped") + get("todo"),
            tool: "node",
        });
    }
    cargo.or(dotnet).or(jest).or(pytest).or(mocha)
}

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
    let cleaned: Vec<String> = output.lines().map(|l| plain(l).into_owned()).collect();
    let all: Vec<&str> = cleaned.iter().map(String::as_str).collect();
    // a suite's own result line is not a test: node's spec opens one with
    // "▶ math" and ends it with "✔ math (2ms)"; in TAP it is the "ok" line
    // less indented than the test line before it
    let mut opened: Vec<String> = Vec::new();
    let mut tap_indent: Option<usize> = None;
    // node's TAP says each result in YAML under its line ("---" to "...")
    let mut yaml: Option<Vec<String>> = None;
    let mut after_tap: Option<(String, bool)> = None;
    // node's spec: "test at math.test.mjs:5:3" above a failing test, the
    // test's name next, then its error
    let mut test_at: Option<(String, Option<String>)> = None;
    let keep = |line: String, with_place: &mut Vec<String>, without: &mut Vec<String>| {
        let list = if locations(&line).is_empty() { without } else { with_place };
        let l: String = line.chars().take(300).collect();
        if !list.contains(&l) {
            list.push(l);
        }
    };
    for (k, raw) in all.iter().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(block) = yaml.as_mut() {
            if line == "..." {
                let block = yaml.take().unwrap_or_default();
                if let Some((name, false)) = &after_tap
                    && let Some(e) = tap_error(&block, name)
                {
                    keep(e, &mut with_place, &mut without);
                }
                after_tap = None;
            } else {
                block.push(line.to_string());
            }
            continue;
        }
        if line == "---" && after_tap.is_some() {
            yaml = Some(Vec::new());
            continue;
        }
        after_tap = None;
        if let Some(name) = line.strip_prefix("▶ ") {
            opened.push(without_time(name));
            continue;
        }
        if let Some(at) = line.strip_prefix("test at ") {
            test_at = Some((at.trim().to_string(), None));
            continue;
        }
        if let Some((name, passed)) = test_line(line) {
            let indent = raw.len() - raw.trim_start().len();
            let tap = line.starts_with("ok ") || line.starts_with("not ok ");
            let ends_suite = if tap {
                let deeper = tap_indent.is_some_and(|i| i > indent);
                tap_indent = Some(indent);
                deeper
            } else {
                // (node lists the failing ones again at the end: a suite there too)
                opened.contains(&name)
            };
            // a suite's YAML says only that its tests failed
            if tap && !ends_suite {
                after_tap = Some((name.clone(), passed));
            } else if tap {
                after_tap = Some((String::new(), true));
            }
            match test_at.as_mut() {
                Some((_, n @ None)) if !passed && !ends_suite => *n = Some(name.clone()),
                _ => test_at = None,
            }
            if ends_suite {
            } else if !passed {
                if !p.failed.contains(&name) {
                    p.failed.push(name);
                }
            } else {
                p.passed += 1;
            }
            continue;
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
            // "Expected values to be strictly equal:": what follows says it
            if line.ends_with(':')
                && let Some(next) = all[k + 1..].iter().map(|n| n.trim()).take(3).find(|n| !n.is_empty())
                && !is_error(next)
                && test_line(next).is_none()
            {
                line = format!("{line} {next}");
            }
            // node's spec: the test's place and name in front
            if locations(&line).is_empty()
                && let Some((at, Some(name))) = test_at.take()
            {
                line = format!("{at}: {name}: {line}");
            }
            keep(line, &mut with_place, &mut without);
        }
    }
    // an error said twice (node lists the failing tests again with their
    // place): once, where it is
    without.retain(|w| !with_place.iter().any(|p| p.contains(w.as_str())));
    p.errors = with_place.into_iter().chain(without).take(ERRORS).collect();
    p
}

/// One error line of a failing test's YAML in node's TAP: "math.test.mjs:5:3:
/// subs: Expected values to be strictly equal: 2 !== 1 (expected 1, got 2)".
fn tap_error(block: &[String], name: &str) -> Option<String> {
    let field = |key: &str| {
        block.iter().position(|l| l.starts_with(&format!("{key}: "))).map(|i| {
            let v = block[i][key.len() + 2..].trim();
            if v == "|-" || v == "|" || v == ">-" {
                // the block under it, up to the next "key: "
                block[i + 1..]
                    .iter()
                    .take_while(|l| !yaml_key(l))
                    .map(|l| l.trim())
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                v.trim_matches('\'').to_string()
            }
        })
    };
    let error = field("error")?;
    let place = field("location").map(|l| {
        let l = l.replace("\\\\", "/").replace('\\', "/");
        let l = l.strip_prefix("file://").unwrap_or(&l).to_string();
        // "/C:/x" is "C:/x"
        let l = if l.as_bytes().get(2) == Some(&b':') { l[1..].to_string() } else { l };
        // under the folder the command ran in: from there, as other tools say it
        let here = std::env::current_dir().map(|d| d.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        match l.get(..here.len()) {
            Some(head) if !here.is_empty() && head.eq_ignore_ascii_case(&here) && l[here.len()..].starts_with('/') => {
                l[here.len() + 1..].to_string()
            }
            _ => l,
        }
    });
    let values = match (field("expected"), field("actual")) {
        (Some(e), Some(a)) if !error.contains(&format!("{a} !== {e}")) => format!(" (expected {e}, got {a})"),
        _ => String::new(),
    };
    Some(match place {
        Some(p) => format!("{p}: {name}: {error}{values}"),
        None => format!("{name}: {error}{values}"),
    })
}

/// "code: 'ERR_ASSERTION'": a key of the YAML, not a line of a value.
fn yaml_key(line: &str) -> bool {
    line.split_once(": ")
        .or_else(|| line.strip_suffix(':').map(|k| (k, "")))
        .is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// A test result line: (test name, passed).
fn test_line(line: &str) -> Option<(String, bool)> {
    let first = |s: &str| s.split_whitespace().next().unwrap_or("").to_string();
    // TAP (node --test off a terminal): "ok 1 - adds", "not ok 2 - subs";
    // "# SKIP" and "# TODO" are neither
    for (mark, passed) in [("ok ", true), ("not ok ", false)] {
        if let Some(r) = line.strip_prefix(mark)
            && let Some((n, rest)) = r.split_once(' ')
            && !n.is_empty()
            && n.chars().all(|c| c.is_ascii_digit())
        {
            let directive = rest.to_uppercase();
            if directive.contains("# SKIP") || directive.contains("# TODO") {
                return None;
            }
            let name = rest.trim_start_matches("- ").split(" # ").next().unwrap_or(rest).trim();
            return Some((name.to_string(), passed));
        }
    }
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
    // jest, vitest, node's spec
    for mark in ["✕ ", "× ", "✗ ", "✖ "] {
        if let Some(r) = line.strip_prefix(mark) {
            // node's "✖ failing tests:" heads the list of them again
            if r.trim_end().ends_with(':') {
                return None;
            }
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
        || lower.starts_with("tests:")
        // node's counts ("ℹ fail 2", TAP "# fail 2") and the list's heading
        || lower.starts_with("ℹ ")
        || lower.starts_with("# ")
        || lower.ends_with("failing tests:");
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
