//! The work flow as agents run it: open a project, record what they learn,
//! finish with a handoff, a new conversation picks it up; moving and
//! removing things.

use std::path::Path;
use std::process::Command;

/// Runs pg as conversation `conv`, in folder `cwd`.
fn pg(home: &Path, cwd: &Path, conv: &str, args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_pg"))
        .args(args)
        .current_dir(cwd)
        .env("PARAGMA_HOME", home)
        .env_remove("PARAGMA_PROJECT")
        .env("PARAGMA_SESSION", conv)
        .output()
        .unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    (o.status.code().unwrap_or(-1), out)
}

fn ok(home: &Path, cwd: &Path, conv: &str, args: &[&str]) -> String {
    let (code, out) = pg(home, cwd, conv, args);
    assert_eq!(code, 0, "pg {args:?}: {out}");
    out
}

#[test]
fn a_new_conversation_continues_where_the_last_one_stopped() {
    let d = tempfile::tempdir().unwrap();
    let (home, code) = (d.path().join("home"), d.path().join("dev/shop-api"));
    std::fs::create_dir_all(code.join("src")).unwrap();

    let out = ok(
        &home,
        &code,
        "a",
        &["open", "Shop API", "--goal", "Refunds via Iyzico", "--step", "Add the client", "--step", "Move refunds"],
    );
    assert!(out.starts_with("ok shop-api (new project)\n"), "{out}");
    assert!(out.contains("- [ ] 2. Move refunds"), "{out}");
    ok(&home, &code, "a", &["write", "--rule", "Amounts are int64 kuruş, never float.", "--group", "Money"]);
    // without --done nothing is ticked off; finishing again replaces the handoff
    let out = ok(&home, &code, "a", &["finish", "Client half done."]);
    assert_eq!(
        out,
        "ok shop-api: 0 steps closed, handoff saved\nopen steps: 1. Add the client; 2. Move refunds (a done one: pg finish again with --done <number>; it replaces this handoff)\n"
    );
    let out =
        ok(&home, &code, "a", &["finish", "Client added; refunds next.", "--done", "1", "--left", "Write the docs"]);
    assert!(
        out.starts_with("ok shop-api: 1 step closed, 1 step left open, handoff replaced\nopen steps: 2. Move refunds; 3. Write the docs"),
        "{out}"
    );

    // another conversation, in a folder below, without a name or by the folder's name
    let out = ok(&home, &code.join("src"), "b", &["open"]);
    assert!(out.starts_with("ok shop-api (continued)\n"), "{out}");
    assert!(out.contains("steps: 2 open, 1 done\n- [ ] 2. Move refunds\n- [ ] 3. Write the docs"), "{out}");
    assert!(out.contains("Client added; refunds next.\nWritten to memory: r1"), "{out}");
    assert!(!out.contains("Client half done."), "{out}");
    let out = ok(&home, &code.join("src"), "c", &["open", "shop api"]);
    assert!(out.starts_with("ok shop-api (continued: same name as \"shop api\")"), "{out}");
    assert!(out.contains("other conversations at work, not finished: shop-api"), "{out}");

    // a similar name is a new project, with a mention
    let out = ok(&home, &code, "d", &["open", "shop"]);
    assert!(out.starts_with("ok shop (new project)\nsimilar project: shop-api (2 open steps)"), "{out}");
    // no name and no link: say how
    let (code_, out) = pg(&home, d.path(), "e", &["open"]);
    assert_eq!(code_, 2, "{out}");
    assert!(out.contains("no project is linked to this folder"), "{out}");
}

#[test]
fn records_are_kept_once_and_contradictions_pointed_out() {
    let d = tempfile::tempdir().unwrap();
    let (home, code) = (d.path().join("home"), d.path().join("shop"));
    std::fs::create_dir_all(&code).unwrap();
    ok(&home, &code, "a", &["open", "shop"]);

    assert_eq!(
        ok(&home, &code, "a", &["write", "--rule", "Round half up, once, at the last step."]),
        "ok r1 (rule created)\n"
    );
    let out = ok(&home, &code, "a", &["write", "--rule", "Rounding is half up, once, at the last step."]);
    assert_eq!(out, "ok r1 (rule updated: the same rule as before)\n");

    ok(&home, &code, "a", &["write", "--decision", "Discounts apply before VAT.", "--why", "finance"]);
    let out = ok(&home, &code, "a", &["write", "--decision", "Discounts do not apply before VAT."]);
    assert!(out.starts_with("ok d2 (decision created)\nmay contradict d1"), "{out}");
    let out =
        ok(&home, &code, "a", &["write", "--decision", "Discounts do not apply before VAT.", "--supersedes", "d1"]);
    assert!(out.contains("d1 is now superseded by d3"), "{out}");
    let d1 = std::fs::read_to_string(home.join("projects/shop/decisions/discounts-apply-before-vat.md")).unwrap();
    assert!(d1.contains("status: superseded\n") && d1.contains("superseded_by: d3\n"), "{d1}");

    let line = r"C:\dev\shop\catalog\catalog_test.go:8: checksum mismatch: got 8c77cb095cbd want 4ff52f25fc9d";
    ok(
        &home,
        &code,
        "a",
        &[
            "write",
            "--error",
            line,
            "--cause",
            "CRLF",
            "--fix",
            "Normalise line endings",
            "--files",
            "catalog/catalog.go",
        ],
    );
    let again = "catalog_test.go:8: checksum mismatch: got 0000aaaa1111 want 4ff52f25fc9d";
    let out = ok(&home, &code, "a", &["write", "--error", again, "--files", "catalog/other.go"]);
    assert_eq!(out, "ok e1 (error updated: the same error, seen 2 times)\n");
    let e1 =
        std::fs::read_to_string(home.join("projects/shop/errors/catalog-test-go-checksum-mismatch-got.md")).unwrap();
    assert!(
        e1.contains("files: [catalog/catalog.go, catalog/other.go]") && e1.contains("## Fix\nNormalise line endings"),
        "{e1}"
    );

    // the record flags need a project, and take no path
    let (c, out) = pg(&home, d.path(), "a", &["write", "--rule", "x"]);
    assert_eq!(c, 2, "{out}");
}

#[test]
fn moving_and_removing() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let cwd = d.path();
    let shop = |args: &[&str]| -> String {
        let mut all = vec!["-p", "shop"];
        all.extend_from_slice(args);
        ok(&home, cwd, "a", &all)
    };
    ok(&home, cwd, "a", &["write", "shop/"]);
    ok(&home, cwd, "a", &["write", "billing/"]);
    shop(&["write", "notes/provider", "Iyzico keys are in the vault."]);
    shop(&["write", "notes/refunds", "See [[provider]]."]);

    assert_eq!(shop(&["mv", "notes/provider", "payments/iyzico"]), "ok shop/notes/provider -> shop/payments/iyzico\n");
    assert!(std::fs::read_to_string(home.join("projects/shop/notes/refunds.md")).unwrap().contains("See [[iyzico]]."));
    assert_eq!(shop(&["rename", "n2", "payments/"]), "ok shop/notes/refunds -> shop/payments/refunds\n");
    assert!(shop(&["read", "n1"]).contains("Iyzico keys"));

    // into another project that already has the id
    ok(&home, cwd, "a", &["-p", "billing", "write", "notes/x", "x"]);
    let out = shop(&["mv", "payments/iyzico", "billing/notes/"]);
    assert!(out.contains("n1 was taken in billing: notes/iyzico is now n2"), "{out}");

    // removed things are kept in .archive/ and come back with mv
    shop(&["write", "--rule", "Amounts are int64."]);
    assert_eq!(shop(&["rm", "r1"]), "ok removed r1 (kept in .archive/rules)\n");
    assert!(!shop(&["read", "rules"]).contains("int64"));
    shop(&["rm", "payments/"]);
    assert_eq!(pg(&home, cwd, "a", &["-p", "shop", "read", "n2"]).0, 3);
    shop(&["mv", ".archive/payments", "payments"]);
    assert!(shop(&["read", "n2"]).contains("See [[iyzico]]."));

    assert_eq!(ok(&home, cwd, "a", &["mv", "billing", "invoicing"]), "ok project billing -> invoicing\n");
    ok(&home, cwd, "a", &["rm", "invoicing"]);
    assert!(home.join("projects/.archive/invoicing/notes/x.md").is_file());
    let ls = ok(&home, cwd, "a", &["ls"]);
    assert!(ls.starts_with("ok 1 project\nshop  "), "{ls}");
}

#[test]
fn notes_unused_for_three_months_are_archived_at_finish() {
    let d = tempfile::tempdir().unwrap();
    let (home, code) = (d.path().join("home"), d.path().join("shop"));
    std::fs::create_dir_all(&code).unwrap();
    ok(&home, &code, "a", &["open", "shop"]);
    ok(&home, &code, "a", &["write", "notes/old", "The staging box is called kiwi."]);
    ok(&home, &code, "a", &["write", "notes/new", "The staging box is called mango."]);
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(100 * 86_400);
    let f = std::fs::File::options().write(true).open(home.join("projects/shop/notes/old.md")).unwrap();
    f.set_modified(old).unwrap();
    drop(f);

    let out = ok(&home, &code, "a", &["finish", "Looked around."]);
    assert!(out.contains("archived (unused for 3 months; still found when searched for): n1\n"), "{out}");
    let note = std::fs::read_to_string(home.join("projects/shop/notes/old.md")).unwrap();
    assert!(note.contains("status: archived"), "{note}");
    // still found, once a day is enough
    assert!(ok(&home, &code, "a", &["find", "kiwi"]).contains("[n1 note]"));
    assert!(!ok(&home, &code, "a", &["finish", "Again."]).contains("archived"));
}
