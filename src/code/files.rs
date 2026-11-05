//! The files of a code folder: what git tracks (and untracked files git
//! does not ignore), or, outside git, every file below except build output
//! and dependencies.

use std::path::Path;
use std::process::Command;

/// Folders that hold build output or dependencies, never written code.
const SKIP: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "vendor",
    "__pycache__",
    "venv",
    "bin",
    "obj",
    "Pods",
    "DerivedData",
];

/// Files larger than this are generated or data.
pub const MAX_SIZE: u64 = 1_000_000;
/// Files read from one folder at most.
const MAX_FILES: usize = 30_000;

/// Paths below `dir` with '/', of the files worth showing: code in a known
/// language, and the usual project files around it.
pub fn list(dir: &Path) -> Vec<String> {
    let mut out = if dir.join(".git").exists() { from_git(dir) } else { None }.unwrap_or_else(|| {
        let mut v = Vec::new();
        walk(dir, dir, &mut v);
        v
    });
    out.retain(|p| shown(p));
    out.sort();
    out.truncate(MAX_FILES);
    out
}

fn from_git(dir: &Path) -> Option<Vec<String>> {
    let o = Command::new("git")
        .args(["-c", "core.quotepath=off", "ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .current_dir(dir)
        .output()
        .ok()?;
    if !o.status.success() {
        return None;
    }
    let mut v: Vec<String> =
        String::from_utf8_lossy(&o.stdout).split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect();
    // a file deleted but still in the git index is found gone when it is
    // read (code/mod.rs), not looked up here a second time
    v.dedup();
    drop_not_own(dir, &mut v);
    Some(v)
}

/// The attributes that say a file is not the project's own code: someone
/// else's (`linguist-vendored`) or made by a tool (`linguist-generated`),
/// as `.gitattributes` marks them for GitHub. Test data and copied
/// libraries marked so stay out of the brief and of `pg find`.
const NOT_OWN: [&str; 2] = ["linguist-vendored", "linguist-generated"];

fn drop_not_own(dir: &Path, files: &mut Vec<String>) {
    // git is asked only when an attributes file names one of them
    let marks = |p: &Path| std::fs::read_to_string(p).is_ok_and(|s| NOT_OWN.iter().any(|a| s.contains(a)));
    let named = files.iter().any(|f| (f == ".gitattributes" || f.ends_with("/.gitattributes")) && marks(&dir.join(f)))
        || marks(&dir.join(".git/info/attributes"));
    if !named {
        return;
    }
    let Some(not_own) = not_own(dir, files) else { return };
    files.retain(|f| !not_own.contains(f));
}

/// The files git gives one of `NOT_OWN`; None when git could not say.
fn not_own(dir: &Path, files: &[String]) -> Option<std::collections::HashSet<String>> {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(["-c", "core.quotepath=off", "check-attr", "-z", "--stdin"])
        .args(NOT_OWN)
        .current_dir(dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let input: Vec<u8> = files.iter().flat_map(|f| f.bytes().chain([0])).collect();
    // written beside the read: a long list would fill both pipes
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = child.wait_with_output().ok()?;
    writer.join().ok()?.ok()?;
    if !out.status.success() {
        return None;
    }
    // path NUL attribute NUL value NUL, for each path and attribute
    let text = String::from_utf8_lossy(&out.stdout);
    let parts: Vec<&str> = text.split('\0').collect();
    Some(
        parts
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|[_, _, v]| *v == "set" || *v == "true")
            .map(|[p, _, _]| p.to_string())
            .collect(),
    )
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || out.len() >= MAX_FILES {
            continue;
        }
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            if !SKIP.contains(&name.as_str()) {
                walk(root, &e.path(), out);
            }
        } else if t.is_file()
            && let Ok(rel) = e.path().strip_prefix(root)
        {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Code in a known language, and the files that say how a project builds
/// and runs.
fn shown(path: &str) -> bool {
    if super::lang::Lang::of(path).is_some() {
        return true;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(
        ext.as_str(),
        "json"
            | "yaml"
            | "yml"
            | "toml"
            | "sql"
            | "html"
            | "css"
            | "scss"
            | "vue"
            | "svelte"
            | "proto"
            | "graphql"
            | "gradle"
            | "xml"
            | "tscn"
    ) || matches!(
        name,
        "Makefile" | "Dockerfile" | "CMakeLists.txt" | "Gemfile" | "Podfile" | "go.mod" | "requirements.txt"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_marked_vendored_or_generated_are_not_listed() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path();
        let write = |p: &str, s: &str| {
            let f = dir.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, s).unwrap();
        };
        write("src/main.rs", "fn main() {}\n");
        write("bench/memory/errors/fake.md", "# A made up failure\n");
        write("bench/run.py", "print(1)\n");
        write("bench/keep/own.py", "print(2)\n");
        write("api/schema.gen.go", "package api\n");
        write("README.md", "# Readme\n");
        write(
            ".gitattributes",
            "* text=auto\nbench/** linguist-vendored\nbench/keep/** -linguist-vendored\n*.gen.go linguist-generated=true\n",
        );
        let git = |args: &[&str]| {
            Command::new("git").args(args).current_dir(dir).output().unwrap();
        };
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        let listed = list(dir);
        assert_eq!(listed, ["README.md", "bench/keep/own.py", "src/main.rs"], "{listed:?}");

        // without such marks nothing is left out, and git is not asked
        write(".gitattributes", "* text=auto\n");
        assert!(list(dir).contains(&"bench/run.py".to_string()));
    }
}
