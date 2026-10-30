//! What each command does, end to end, as an agent sees it.

use std::path::Path;
use std::process::Command;

struct Run {
    code: i32,
    out: String,
}

fn pg(home: &Path, args: &[&str]) -> Run {
    let o = Command::new(env!("CARGO_BIN_EXE_pg"))
        .args(args)
        .env("PARAGMA_HOME", home)
        .env_remove("PARAGMA_PROJECT")
        .env("PARAGMA_SESSION", "test")
        .output()
        .unwrap();
    Run {
        code: o.status.code().unwrap_or(-1),
        out: format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
    }
}

fn ok(home: &Path, args: &[&str]) -> String {
    let r = pg(home, args);
    assert_eq!(r.code, 0, "pg {args:?}: {}", r.out);
    r.out
}

#[test]
fn projects_folders_and_notes() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    assert_eq!(ok(h, &["write", "game-engine/"]), "ok game-engine (project)\n");
    assert_eq!(ok(h, &["-p", "game-engine", "write", "render/"]), "ok game-engine/render/ (folder)\n");
    assert!(h.join("projects/game-engine/render").is_dir());

    let out = ok(h, &["-p", "game-engine", "write", "physics/collisions", "Use swept AABB for fast bodies."]);
    assert_eq!(out, "ok game-engine/physics/collisions (n1, created)\n");
    let file = std::fs::read_to_string(h.join("projects/game-engine/physics/collisions.md")).unwrap();
    assert_eq!(file, "---\nid: n1\ntype: note\ntitle: collisions\n---\nUse swept AABB for fast bodies.\n");

    // update keeps the id, append adds below
    assert!(ok(h, &["-p", "game-engine", "write", "physics/collisions", "Use swept AABB."]).contains("(n1, updated)"));
    assert!(
        ok(h, &["-p", "game-engine", "write", "physics/collisions", "--append", "Only above 20 m/s."])
            .contains("(n1, appended)")
    );
    let read = ok(h, &["-p", "game-engine", "read", "n1"]);
    assert_eq!(read, "[n1 note] physics/collisions\nUse swept AABB.\n\nOnly above 20 m/s.\n");
    assert_eq!(ok(h, &["-p", "game-engine", "read", "physics/collisions"]), read);

    let ls = ok(h, &["-p", "game-engine", "ls"]);
    assert!(ls.contains("physics/  1 note\n") && ls.contains("render/  0 notes\n"), "{ls}");
    assert!(ok(h, &["ls"]).contains("game-engine  1 note"));
}

#[test]
fn another_project_by_name_and_search_across() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    ok(h, &["write", "shop/payments/refunds", "Refunds are allowed within 30 days of the payment."]);
    ok(h, &["write", "game/physics", "Fixed time step of 60 Hz for the physics loop."]);
    // from inside one project, a path can name another
    assert!(
        ok(h, &["-p", "game", "write", "shop/payments/vat", "VAT is rounded half up."])
            .starts_with("ok shop/payments/vat")
    );

    // the game project holds nothing about refunds
    let here = pg(h, &["-p", "game", "find", "refund window"]);
    assert_eq!(here.code, 1, "searched outside the project: {}", here.out);
    let there = ok(h, &["-p", "game", "find", "refund window", "--in", "shop"]);
    assert!(there.contains("Refunds are allowed"), "{there}");
    let all = ok(h, &["find", "refund window"]);
    assert!(all.contains("(all projects)") && all.contains("[shop:n1 note]"), "{all}");
}

#[test]
fn exit_codes() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    ok(h, &["write", "p/notes/a", "Shaders use half floats on mobile."]);
    assert_eq!(pg(h, &["-p", "p", "find", "sourdough bread baking"]).code, 1, "nothing found is exit 1");
    assert_eq!(pg(h, &["-p", "p", "read", "n99"]).code, 3);
    assert_eq!(pg(h, &["-p", "nope", "ls"]).code, 3);
    assert_eq!(pg(h, &["-p", "p", "write", "bad:name", "x"]).code, 2);
    assert_eq!(pg(h, &["doctor"]).code, 2, "doctor needs a project");
}

#[test]
fn notes_edited_outside_pg_are_seen() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    ok(h, &["write", "p/a", "first text"]);
    // an editor (or Obsidian) rewrites the file and adds a new one
    std::fs::write(h.join("projects/p/a.md"), "---\nid: n1\ntype: note\n---\nShaders use half floats on mobile.\n")
        .unwrap();
    std::fs::write(h.join("projects/p/b.md"), "# Physics\nFixed time step of 60 Hz.\n").unwrap();
    assert!(ok(h, &["-p", "p", "find", "half floats shaders"]).contains("[n1 note] a"));
    assert!(ok(h, &["-p", "p", "read", "b"]).contains("Fixed time step"));
    std::fs::remove_file(h.join("projects/p/b.md")).unwrap();
    assert!(ok(h, &["-p", "p", "doctor"]).contains(" 1 note,"));
}
