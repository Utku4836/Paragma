//! What agents and real projects throw at pg: many commands of every kind
//! at once from several conversations, and files that are not what pg
//! expects. Every command must end well (or say why), nothing may be lost.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn pg(home: &Path, cwd: &Path, conv: &str, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_pg"));
    c.args(args).current_dir(cwd).env("PARAGMA_HOME", home).env_remove("PARAGMA_PROJECT").env("PARAGMA_SESSION", conv);
    c.stdout(Stdio::piped()).stderr(Stdio::piped());
    c
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn ok(home: &Path, cwd: &Path, args: &[&str]) -> String {
    let o = pg(home, cwd, "main", args).output().unwrap();
    assert!(o.status.success(), "pg {args:?}: {}", text(&o));
    text(&o)
}

/// A git repository with some Go code, opened as project shop.
fn shop(d: &Path) -> (PathBuf, PathBuf) {
    let (home, code) = (d.join("home"), d.join("shop"));
    std::fs::create_dir_all(code.join("payments")).unwrap();
    std::fs::write(code.join("go.mod"), "module shop\n").unwrap();
    for i in 0..20 {
        let src =
            format!("package payments\n\n// F{i} does step {i}.\nfunc F{i}(a int) int {{\n\treturn a + {i}\n}}\n");
        std::fs::write(code.join(format!("payments/f{i}.go")), src).unwrap();
    }
    let git =
        |args: &[&str]| assert!(Command::new("git").args(args).current_dir(&code).output().unwrap().status.success());
    git(&["init", "-q"]);
    git(&["-c", "user.email=t@t", "-c", "user.name=t", "add", "-A"]);
    git(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);
    ok(&home, &code, &["open", "shop", "--goal", "Payments steps"]);
    (home, code)
}

#[test]
fn every_kind_of_command_at_once_from_several_conversations() {
    let d = tempfile::tempdir().unwrap();
    let (home, code) = shop(d.path());
    ok(&home, &code, &["write", "notes/move-me", "to be moved"]);
    ok(&home, &code, &["write", "notes/remove-me", "to be removed"]);
    let cmd = "echo ok";

    let mut children: Vec<(String, std::process::Child)> = Vec::new();
    let mut spawn = |what: String, conv: &str, args: Vec<String>| {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        children.push((what, pg(&home, &code, conv, &args).spawn().unwrap()));
    };
    for i in 0..6 {
        let conv = format!("conv{}", i % 3);
        spawn(format!("open {i}"), &conv, vec!["open".into(), "--step".into(), format!("step {i}")]);
        spawn(
            format!("rule {i}"),
            &conv,
            vec!["write".into(), "--rule".into(), format!("Rule number {i} about payments step {i}.")],
        );
        spawn(
            format!("error {i}"),
            &conv,
            vec![
                "write".into(),
                "--error".into(),
                format!(
                    "f{i}.go:5: {}",
                    [
                        "nil pointer dereference",
                        "checksum mismatch in catalog",
                        "context deadline exceeded",
                        "connection refused by ledger",
                        "invalid currency code",
                        "duplicate refund id"
                    ][i]
                ),
                "--fix".into(),
                format!("fix {i}"),
            ],
        );
        spawn(
            format!("decision {i}"),
            &conv,
            vec![
                "write".into(),
                "--decision".into(),
                format!("Use plan {} for area {i}.", ["A", "B", "C", "D", "E", "F"][i]),
                "--title".into(),
                format!("Area {i} plan"),
            ],
        );
        spawn(format!("note {i}"), &conv, vec!["write".into(), format!("notes/n{i}"), format!("note {i}")]);
        spawn(format!("find {i}"), &conv, vec!["find".into(), "payments step".into()]);
        spawn(format!("tree {i}"), &conv, vec!["tree".into()]);
        spawn(format!("read {i}"), &conv, vec!["read".into(), format!("payments/f{i}.go#F{i}")]);
        spawn(format!("run {i}"), &conv, vec!["run".into(), "--".into(), cmd.into()]);
        spawn(
            format!("secret {i}"),
            &conv,
            vec!["write".into(), "--secret".into(), format!("Service {i}"), format!("value-{i}-abcdefgh")],
        );
    }
    spawn("mv".into(), "conv0", vec!["mv".into(), "notes/move-me".into(), "moved/".into()]);
    spawn("rm".into(), "conv1", vec!["rm".into(), "notes/remove-me".into()]);
    spawn("finish".into(), "conv2", vec!["finish".into(), "Parallel work.".into()]);

    for (what, child) in children {
        let out = child.wait_with_output().unwrap();
        let code = out.status.code().unwrap_or(-1);
        let fine = code == 0 || (what.starts_with("find") && code == 1);
        assert!(fine, "{what} exited {code}: {}", text(&out));
    }

    // nothing lost, ids unique
    let rules = std::fs::read_to_string(home.join("projects/shop/rules.md")).unwrap();
    let ids: HashSet<&str> = rules.lines().filter_map(|l| l.strip_prefix("- ")?.split(':').next()).collect();
    assert_eq!(ids.len(), 6, "{rules}");
    let count = |dir: &str| std::fs::read_dir(home.join("projects/shop").join(dir)).map(|d| d.count()).unwrap_or(0);
    assert_eq!((count("errors"), count("decisions")), (6, 6));
    let project = std::fs::read_to_string(home.join("projects/shop/project.md")).unwrap();
    for i in 0..6 {
        assert!(project.contains(&format!("step {i}")), "step {i} lost:\n{project}");
    }
    let env = std::fs::read_to_string(home.join("projects/shop/shop.env")).unwrap();
    assert_eq!(env.matches("SERVICE_").count(), 6, "{env}");
    assert!(home.join("projects/shop/moved/move-me.md").is_file());
    assert!(home.join("projects/shop/.archive/notes/remove-me.md").is_file());
    let doctor = ok(&home, &code, &["doctor"]);
    assert!(doctor.contains("code: 21 files, 20 symbols"), "{doctor}");
    let notes: HashSet<String> = ok(&home, &code, &["ls", "errors"])
        .lines()
        .skip(1)
        .map(|l| l.split_whitespace().nth(1).unwrap_or("").to_string())
        .collect();
    assert_eq!(notes.len(), 6, "error ids not unique: {notes:?}");
}

#[test]
fn files_that_are_not_what_pg_expects() {
    let d = tempfile::tempdir().unwrap();
    let (home, code) = shop(d.path());
    let notes = home.join("projects/shop/notes");
    std::fs::create_dir_all(&notes).unwrap();
    // Windows line endings, a byte order mark, UTF-16, empty, only properties, huge, not UTF-8
    std::fs::write(
        notes.join("crlf.md"),
        "---\r\nid: n90\r\ntype: decision\r\ntitle: Line endings\r\n---\r\nWe keep CRLF out of testdata.\r\n",
    )
    .unwrap();
    std::fs::write(notes.join("bom.md"), "\u{feff}# Kiwi box\nThe staging box is kiwi.\n").unwrap();
    let utf16: Vec<u8> = "\u{feff}UTF-16 note about mango".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    std::fs::write(notes.join("utf16.md"), utf16).unwrap();
    std::fs::write(notes.join("empty.md"), "").unwrap();
    std::fs::write(notes.join("props.md"), "---\nid: n91\ntype: note\n---\n").unwrap();
    std::fs::write(notes.join("huge.md"), format!("## Big\n{}\n", "refund window detail. ".repeat(250_000))).unwrap();
    std::fs::write(notes.join("latin1.md"), b"caf\xe9 au lait\n").unwrap();
    std::fs::create_dir_all(notes.join("Ödeme Kuralları")).unwrap();
    std::fs::write(notes.join("Ödeme Kuralları/İade süresi.md"), "İade süresi 30 gündür.\n").unwrap();
    // code that is not code: binary, not UTF-8, a symlink loop
    std::fs::write(code.join("payments/blob.go"), [0u8, 159, 146, 150, 0, 1, 2, 255]).unwrap();
    std::fs::write(code.join("payments/latin1.go"), b"package payments\n// caf\xe9\nfunc Cafe() {}\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&code, code.join("payments/loop")).unwrap();

    let doctor = ok(&home, &code, &["doctor"]);
    assert!(doctor.starts_with("ok shop: "), "{doctor}");
    assert!(ok(&home, &code, &["find", "line endings testdata"]).contains("[n90 decision]"));
    assert!(ok(&home, &code, &["find", "staging box kiwi"]).contains("Kiwi box"));
    assert!(ok(&home, &code, &["find", "iade süresi"]).contains("30 gündür"));
    ok(&home, &code, &["read", "notes/huge"]);
    ok(&home, &code, &["read", "notes/latin1"]);
    ok(&home, &code, &["tree"]);
    ok(&home, &code, &["ls", "notes"]);
    ok(&home, &code, &["open"]);
    ok(&home, &code, &["finish", "Survived odd files."]);
}

#[test]
fn the_first_code_index_built_by_many_at_once() {
    let d = tempfile::tempdir().unwrap();
    let (home, code) = shop(d.path());
    std::fs::remove_file(home.join("projects/shop/.pg/code.db")).ok();
    let children: Vec<_> = (0..12).map(|i| pg(&home, &code, &format!("c{i}"), &["tree"]).spawn().unwrap()).collect();
    for c in children {
        let out = c.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", text(&out));
    }
    assert!(ok(&home, &code, &["tree"]).starts_with("ok shop: 21 files, 20 symbols"));
}
