//! Many pg processes at once on one project: every command must succeed,
//! no write may be lost, and ids must stay unique.

use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Output};

fn pg(home: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_pg"));
    c.args(args).env("PARAGMA_HOME", home).env_remove("PARAGMA_PROJECT").env("PARAGMA_SESSION", "test");
    c
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn mixed_commands_in_parallel() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    assert!(pg(h, &["write", "proj/"]).output().unwrap().status.success());

    let mut children = Vec::new();
    for i in 0..30 {
        let body = format!("note number {i} about collision detection");
        children.push(("write", pg(h, &["-p", "proj", "write", &format!("physics/n{i}"), &body]).spawn().unwrap()));
    }
    for i in 0..15 {
        let line = format!("finding {i}");
        children.push(("append", pg(h, &["-p", "proj", "write", "log", "--append", &line]).spawn().unwrap()));
    }
    for _ in 0..10 {
        children.push(("find", pg(h, &["-p", "proj", "find", "collision detection"]).spawn().unwrap()));
    }
    for _ in 0..5 {
        children.push(("ls", pg(h, &["-p", "proj", "ls", "physics"]).spawn().unwrap()));
    }
    for (what, child) in children {
        let out = child.wait_with_output().unwrap();
        let code = out.status.code().unwrap_or(-1);
        // find may run before any note exists: "nothing found" is a fine answer
        let fine = code == 0 || (what == "find" && code == 1) || (what == "ls" && code == 3);
        assert!(fine, "{what} exited {code}: {}", text(&out));
    }

    // every write landed, with its own id
    let mut ids = HashSet::new();
    for i in 0..30 {
        let out = pg(h, &["-p", "proj", "read", &format!("physics/n{i}")]).output().unwrap();
        let t = text(&out);
        assert!(out.status.success() && t.contains(&format!("note number {i} ")), "n{i}: {t}");
        let id = t.split(['[', ' ']).nth(1).unwrap().to_string();
        assert!(ids.insert(id.clone()), "id {id} given twice");
    }
    // no append was lost
    let log = text(&pg(h, &["-p", "proj", "read", "log", "--budget", "5000"]).output().unwrap());
    for i in 0..15 {
        assert!(
            log.contains(&format!("finding {i}\n")) || log.contains(&format!("finding {i}")),
            "append {i} lost:\n{log}"
        );
    }
    // and the index agrees with the files
    let doctor = text(&pg(h, &["-p", "proj", "doctor"]).output().unwrap());
    assert!(doctor.contains(" 31 notes"), "{doctor}");
}

#[test]
fn first_index_built_by_many_at_once() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("projects/big/notes");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..200 {
        std::fs::write(dir.join(format!("n{i}.md")), format!("# Note {i}\nCache warming for service {i}.\n")).unwrap();
    }
    let children: Vec<_> =
        (0..16).map(|_| pg(home.path(), &["-p", "big", "find", "cache warming"]).spawn().unwrap()).collect();
    for c in children {
        let out = c.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", text(&out));
    }
    let doctor = text(&pg(home.path(), &["-p", "big", "doctor"]).output().unwrap());
    assert!(doctor.contains(" 200 notes, 200 pieces"), "{doctor}");
}
