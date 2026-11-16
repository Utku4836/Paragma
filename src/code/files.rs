//! The files of a code folder: what git tracks (and untracked files git
//! does not ignore), or, outside git, every file below except build output
//! and dependencies.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

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

/// What `list` found in this run of pg, by folder: a command can open the
/// code index more than once (the brief: stale notes, then the code part),
/// and git is slow to start (about 0.1 s on Windows), so the folder is
/// listed once, and early (`list_ahead`) while other work goes on.
type Listed = Arc<OnceLock<Vec<String>>>;
static LISTED: Mutex<Vec<(PathBuf, Listed)>> = Mutex::new(Vec::new());

fn listed(dir: &Path) -> Listed {
    let mut all = LISTED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, l)) = all.iter().find(|(d, _)| d == dir) {
        return l.clone();
    }
    let l = Listed::default();
    all.push((dir.to_path_buf(), l.clone()));
    l
}

/// Paths below `dir` with '/', of the files worth showing: code in a known
/// language, and the usual project files around it. Listed once a run of
/// pg (`forget` after something that may change the files).
pub fn list(dir: &Path) -> Vec<String> {
    listed(dir).get_or_init(|| list_now(dir)).clone()
}

/// Starts listing these folders in the background, for a `list` later in
/// this run.
pub fn list_ahead<P: AsRef<Path>>(dirs: impl IntoIterator<Item = P>) {
    for d in dirs {
        let dir = d.as_ref().to_path_buf();
        if !dir.is_dir() {
            continue;
        }
        let l = listed(&dir);
        if l.get().is_none() {
            std::thread::spawn(move || {
                l.get_or_init(|| list_now(&dir));
            });
        }
    }
}

/// Lists afresh from now on: after a command that may have made or removed
/// files (pg run).
pub fn forget() {
    LISTED.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

fn list_now(dir: &Path) -> Vec<String> {
    if dir.join(".git").is_dir() {
        if let Some(files) = kept::load(dir) {
            return files;
        }
        if let Some(all) = from_git(dir) {
            let files = worth_showing(all.clone());
            kept::store(dir, &all, &files);
            return files;
        }
    }
    let mut v = Vec::new();
    walk(dir, dir, &mut v);
    worth_showing(v)
}

fn worth_showing(mut files: Vec<String>) -> Vec<String> {
    files.retain(|p| shown(p));
    files.sort();
    files.truncate(MAX_FILES);
    files
}

/// Where listings of git folders are kept between runs of pg (set once a
/// run; unset: not kept).
static KEPT_IN: OnceLock<PathBuf> = OnceLock::new();

pub fn keep_listings_in(dir: PathBuf) {
    let _ = KEPT_IN.set(dir);
}

/// A git folder's listing kept between runs: listing asks git twice (the
/// files, then their attributes), about 0.2 s on Windows, for every command
/// that reads the code (the brief, a hook before each edit). The listing is
/// used again while nothing that decides it changed: git's index (a file
/// added, removed, checked out), the ignore and attribute files, and the
/// folders holding the files (a file made or removed changes its folder's
/// time). At most an hour old, for what this cannot see (git's global
/// settings).
mod kept {
    use std::hash::{Hash, Hasher};
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    /// Folders checked at most; a larger tree is listed by git every time.
    const MOST_FOLDERS: usize = 5_000;
    const MOST_AGE: Duration = Duration::from_secs(3600);

    fn file(dir: &Path) -> Option<std::path::PathBuf> {
        let root = super::KEPT_IN.get()?;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        dir.hash(&mut h);
        Some(root.join(format!("{:016x}.json", h.finish())))
    }

    /// Every folder holding a file of the listing ("" is the folder itself).
    fn folders(all: &[String]) -> Vec<String> {
        let mut v: Vec<String> =
            all.iter().map(|p| p.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default()).collect();
        v.push(String::new());
        v.sort();
        v.dedup();
        v
    }

    fn time(p: &Path) -> u128 {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }

    /// What decides the listing, as one number.
    fn stamp(dir: &Path, folders: &[String]) -> String {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        env!("CARGO_PKG_VERSION").hash(&mut h);
        for f in [".git/index", ".git/info/exclude", ".git/info/attributes"] {
            time(&dir.join(f)).hash(&mut h);
        }
        for f in folders {
            let d = if f.is_empty() { dir.to_path_buf() } else { dir.join(f) };
            f.hash(&mut h);
            time(&d).hash(&mut h);
            time(&d.join(".gitignore")).hash(&mut h);
            time(&d.join(".gitattributes")).hash(&mut h);
        }
        format!("{:016x}", h.finish())
    }

    pub fn load(dir: &Path) -> Option<Vec<String>> {
        let path = file(dir)?;
        let age = SystemTime::now().duration_since(std::fs::metadata(&path).ok()?.modified().ok()?).ok()?;
        if age > MOST_AGE {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
        let strings = |k: &str| -> Option<Vec<String>> {
            v[k].as_array()?.iter().map(|x| x.as_str().map(str::to_string)).collect()
        };
        if v["dir"].as_str()? != dir.to_string_lossy() {
            return None;
        }
        let folders = strings("folders")?;
        (v["stamp"].as_str()? == stamp(dir, &folders)).then(|| strings("files")).flatten()
    }

    pub fn store(dir: &Path, all: &[String], files: &[String]) {
        let Some(path) = file(dir) else { return };
        let folders = folders(all);
        if folders.len() > MOST_FOLDERS {
            return;
        }
        let v = serde_json::json!({
            "dir": dir.to_string_lossy(),
            "stamp": stamp(dir, &folders),
            "folders": folders,
            "files": files,
        });
        let _ = crate::project::files::write_atomic(&path, v.to_string().as_bytes());
    }
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
        // (in a new run of pg: a run lists a folder once)
        write(".gitattributes", "* text=auto\n");
        forget();
        assert!(list(dir).contains(&"bench/run.py".to_string()));
    }

    #[test]
    fn a_kept_listing_follows_what_changes() {
        let d = tempfile::tempdir().unwrap();
        let (dir, kept) = (d.path().join("code"), d.path().join("kept"));
        keep_listings_in(kept);
        let write = |p: &str| {
            let f = dir.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, "x\n").unwrap();
        };
        write("src/a.rs");
        write("src/deep/b.rs");
        Command::new("git").args(["init", "-q"]).current_dir(&dir).output().unwrap();
        let run = || {
            forget();
            list(&dir)
        };
        assert_eq!(run(), ["src/a.rs", "src/deep/b.rs"]);
        // untracked files made and removed, in a known folder and a new one
        write("src/deep/c.rs");
        assert_eq!(run(), ["src/a.rs", "src/deep/b.rs", "src/deep/c.rs"]);
        write("src/new/d.rs");
        assert!(run().contains(&"src/new/d.rs".to_string()));
        std::fs::remove_file(dir.join("src/a.rs")).unwrap();
        assert!(!run().contains(&"src/a.rs".to_string()));
        // an ignore file that leaves a folder out
        std::fs::write(dir.join(".gitignore"), "src/new/\n").unwrap();
        assert!(!run().contains(&"src/new/d.rs".to_string()));
        // the listing was kept: the same answer without git, while nothing changed
        if let Some(k) = KEPT_IN.get() {
            assert!(std::fs::read_dir(k).is_ok_and(|mut r| r.next().is_some()));
        }
    }
}
