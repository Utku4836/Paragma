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
    for raw in output.lines() {
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
            let list = if locations(line).is_empty() { &mut without } else { &mut with_place };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suites_that_say_they_passed() {
        assert!(suite_passed("ok  \tshop/payments\t0.012s\nok  \tshop/cart\t(cached)\n"));
        assert!(suite_passed("test result: ok. 12 passed; 0 failed; 0 ignored\n"));
        assert!(suite_passed("============ 5 passed in 0.12s ============\n"));
        assert!(suite_passed("Tests:       5 passed, 5 total\n"));
        assert!(suite_passed("Passed!  - Failed:     0, Passed:     5, Skipped:     0\n"));
        assert!(!suite_passed("--- FAIL: TestRefund (0.00s)\nFAIL\tshop/payments\t0.01s\n"));
        assert!(!suite_passed("Tests:       1 failed, 4 passed, 5 total\n"));
        assert!(!suite_passed("==== 1 failed, 2 passed in 0.3s ====\n"));
        assert!(!suite_passed("ok\nthe check passed\n"));
        assert!(!suite_passed("go.mod\nREADME.md\n"));
    }

    #[test]
    fn suites_that_say_they_failed() {
        assert!(suite_failed("--- FAIL: TestRefund (0.00s)\nFAIL\nFAIL\tshop/payments\t0.01s\n"));
        assert!(suite_failed(
            "# shop/payments\npayments/refund.go:9:3: undefined: x\nFAIL\tshop/payments [build failed]\n"
        ));
        assert!(suite_failed("test result: FAILED. 1 passed; 1 failed;\n"));
        assert!(suite_failed("error: could not compile `shop` (lib) due to 1 previous error\n"));
        assert!(suite_failed("==== 1 failed, 2 passed in 0.3s ====\n"));
        assert!(suite_failed("Tests:       1 failed, 4 passed, 5 total\n"));
        assert!(suite_failed("Failed!  - Failed:     1, Passed:     4\n"));
        // a source file or a log that only looks like trouble
        let source = "func TestRefund(t *testing.T) {\n\tif Refund(-1) == nil {\n\t\tt.Fatal(\"negative days must be rejected\")\n\t}\n}\n";
        assert!(!suite_failed(source));
        assert!(!suite_failed("error: file not found\nretrying, 0 failed so far\n"));
        assert!(!suite_failed("==== 3 passed in 0.2s ====\n"));
    }

    #[test]
    fn go_test() {
        let out = "=== RUN   TestRefund\n    refund_test.go:41: refund window exceeded: got 31 days\n--- FAIL: TestRefund (0.00s)\n--- PASS: TestTotal (0.00s)\nFAIL\nFAIL\tshop/payments\t0.01s\n";
        let p = parse(out);
        assert_eq!(p.failed, vec!["TestRefund"]);
        assert_eq!(p.passed, 1);
        assert_eq!(p.errors, vec!["refund_test.go:41: refund window exceeded: got 31 days"]);
    }

    #[test]
    fn cargo_pytest_jest_dotnet() {
        let p = parse(
            "test cart::total ... ok\ntest cart::refund ... FAILED\nthread 'cart::refund' panicked at src/cart.rs:12:5:\n",
        );
        assert_eq!((p.failed, p.passed), (vec!["cart::refund".to_string()], 1));
        assert_eq!(p.errors, vec!["thread 'cart::refund' panicked at src/cart.rs:12:5:"]);
        let p = parse(
            "tests/test_cart.py::test_total PASSED [ 50%]\nFAILED tests/test_cart.py::test_refund - AssertionError: 31 != 30\n",
        );
        assert_eq!((p.failed, p.passed), (vec!["tests/test_cart.py::test_refund".to_string()], 1));
        let p = parse("  ✓ adds totals (3 ms)\n  ✕ refunds within window (5 ms)\n");
        assert_eq!((p.failed, p.passed), (vec!["refunds within window".to_string()], 1));
        let p = parse("  Failed Shop.Tests.RefundTest [12 ms]\n  Passed Shop.Tests.TotalTest\n");
        assert_eq!((p.failed, p.passed), (vec!["Shop.Tests.RefundTest".to_string()], 1));
    }

    #[test]
    fn places() {
        assert_eq!(locations("refund_test.go:41: refund window"), vec![("refund_test.go".to_string(), 41)]);
        assert_eq!(locations(r"at C:\dev\shop\cart.py:12 in total"), vec![(r"C:\dev\shop\cart.py".to_string(), 12)]);
        assert_eq!(locations("  File \"shop/cart.py\", line 12, in total"), vec![("shop/cart.py".to_string(), 12)]);
        assert_eq!(locations("src/cart.rs:12:5: panicked"), vec![("src/cart.rs".to_string(), 12)]);
    }
}
