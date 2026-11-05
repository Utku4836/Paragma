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
    Some(v)
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
