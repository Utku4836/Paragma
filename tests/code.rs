//! The code side as agents use it: the map, reading a function, runs of
//! the tests, what finish says about changed code, notes following a
//! renamed function.

use std::path::Path;
use std::process::Command;

fn pg(home: &Path, cwd: &Path, args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_pg"))
        .args(args)
        .current_dir(cwd)
        .env("PARAGMA_HOME", home)
        .env_remove("PARAGMA_PROJECT")
        .env("PARAGMA_SESSION", "code-test")
        .output()
        .unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    (o.status.code().unwrap_or(-1), out)
}

fn ok(home: &Path, cwd: &Path, args: &[&str]) -> String {
    let (code, out) = pg(home, cwd, args);
    assert_eq!(code, 0, "pg {args:?}: {out}");
    out
}

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

const REFUND: &str = "package payments\n\n// Refund pays an order back within the window.\nfunc Refund(days int) error {\n\tif days > window() {\n\t\treturn ErrWindow\n\t}\n\treturn nil\n}\n\nfunc window() int { return 30 }\n";
const TEST: &str = "package payments\n\nimport \"testing\"\n\nfunc TestRefund(t *testing.T) {\n\tif Refund(30) != nil {\n\t\tt.Fatal(\"day 30\")\n\t}\n}\n";

/// A code folder under git with a small Go project, opened as project shop.
fn shop() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let (home, code) = (d.path().join("home"), d.path().join("shop"));
    std::fs::create_dir_all(code.join("payments")).unwrap();
    std::fs::write(code.join("go.mod"), "module shop\n").unwrap();
    std::fs::write(code.join("payments/refund.go"), REFUND).unwrap();
    std::fs::write(code.join("payments/refund_test.go"), TEST).unwrap();
    git(&code, &["init", "-q"]);
    git(&code, &["-c", "user.email=t@t", "-c", "user.name=t", "-c", "core.autocrlf=false", "add", "-A"]);
    git(&code, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);
    ok(&home, &code, &["open", "shop", "--goal", "Refunds within a 30 day window"]);
    (d, home, code)
}

#[test]
fn the_map_reading_code_and_finding_it() {
    let (_d, home, code) = shop();
    let tree = ok(&home, &code, &["tree"]);
    assert!(tree.starts_with("ok shop: 3 files, 3 symbols (go 2)\n"), "{tree}");
    assert!(tree.contains("  refund.go  Refund, window\n"), "{tree}");
    let file = ok(&home, &code, &["tree", "payments/refund.go"]);
    assert!(file.contains("function Refund  4-9  func Refund(days int) error {"), "{file}");

    let read = ok(&home, &code, &["read", "refund.go#window"]);
    assert_eq!(read, "[payments/refund.go#window function 11-11]\n  11  func window() int { return 30 }\n");
    assert_eq!(
        ok(&home, &code, &["read", "payments/refund.go:1-1"]),
        "[payments/refund.go:1-1]\n   1  package payments\n"
    );

    // an error line pointing at code is tied to its file and function
    ok(&home, &code, &["write", "--error", "refund_test.go:7: day 30", "--fix", "window is 30 days"]);
    ok(&home, &code, &["write", "--rule", "Windows count whole days. (applies: payments/)"]);
    let found = ok(&home, &code, &["find", "TestRefund"]);
    assert!(found.contains("[code] payments/refund_test.go#TestRefund function 5-9: func TestRefund(t *testing.T) {  [1 rule: r1; 1 error: e1]"), "{found}");
}

#[test]
fn runs_new_failures_known_fixes_and_fixed_ones() {
    let (_d, home, code) = shop();
    // a stand-in for the test command: prints out.txt, exits with code.txt
    let failing = "--- FAIL: TestRefund (0.00s)\n    refund_test.go:7: day 30\nFAIL\n";
    std::fs::write(code.join("out.txt"), failing).unwrap();
    std::fs::write(code.join("code.txt"), "1").unwrap();
    std::fs::write(code.join("t.cmd"), "@type out.txt\r\n@set /p c=<code.txt\r\n@exit /b %c%\r\n").unwrap();
    std::fs::write(code.join("t.sh"), "cat out.txt; exit $(cat code.txt)\n").unwrap();
    let args = ["run", "--", if cfg!(windows) { r".\t.cmd" } else { "sh t.sh" }];
    let (c, out) = pg(&home, &code, &args);
    assert_eq!(c, 1, "{out}");
    assert!(out.contains("1 failing, 0 passed\nnew: TestRefund\nerrors:\nrefund_test.go:7: day 30\n  in payments/refund_test.go#TestRefund (function 5-9)\nonce fixed, remember it: pg write --error"), "{out}");

    ok(&home, &code, &["write", "--error", "refund_test.go:7: day 30", "--fix", "window is 30 days"]);
    let (_, out) = pg(&home, &code, &args);
    assert!(out.contains("failing already: TestRefund (2 runs)"), "{out}");
    assert!(out.contains("known: e1 (seen 1 time): fix: window is 30 days (pg read e1)"), "{out}");

    std::fs::write(code.join("out.txt"), "--- PASS: TestRefund (0.00s)\nPASS\n").unwrap();
    std::fs::write(code.join("code.txt"), "0").unwrap();
    let out = ok(&home, &code, &args);
    assert!(out.contains(": 1 passed\nfixed since the last run: TestRefund\n"), "{out}");
}

#[test]
fn finish_names_the_changed_code_and_its_rules_and_notes_follow_renames() {
    let (_d, home, code) = shop();
    ok(&home, &code, &["write", "--rule", "Windows count whole days. (applies: payments/)"]);
    ok(&home, &code, &["write", "--decision", "The window is 30 days.", "--title", "Refund window"]);
    // a note about the function, by name
    let d1 = home.join("projects/shop/decisions/refund-window.md");
    let text = std::fs::read_to_string(&d1).unwrap().replacen("status: active", "symbols: [window]\nstatus: active", 1);
    std::fs::write(&d1, text).unwrap();

    std::fs::write(code.join("payments/refund.go"), REFUND.replace("days > window()", "days >= window()+1")).unwrap();
    let out = ok(&home, &code, &["finish", "Window check rewritten."]);
    assert!(
        out.contains(
            "changed: payments/refund.go#Refund\ncheck the change against:\nr1 rule: Windows count whole days."
        ),
        "{out}"
    );
    assert!(out.contains("d1 decision: Refund window (via window)"), "{out}");
    let brief = ok(&home, &code, &["open"]);
    assert!(brief.contains("Changed: payments/refund.go#Refund"), "{brief}");
    assert!(brief.contains("tests: pg run -- go test ./..."), "{brief}");

    // the function is renamed: the decision follows it
    std::fs::write(code.join("payments/refund.go"), REFUND.replace("window()", "refundWindow()")).unwrap();
    ok(&home, &code, &["tree"]);
    let d1 = std::fs::read_to_string(&d1).unwrap();
    assert!(d1.contains("symbols: [refundWindow]"), "{d1}");
}
