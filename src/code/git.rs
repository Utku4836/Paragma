//! What git knows about a code folder: what changed since the last commit
//! (by line), and which files usually change together.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new("git").args(["-c", "core.quotepath=off"]).args(args).current_dir(dir).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Logs asked for in this run of pg, by folder and arguments: started early
/// (`log_ahead`) they run beside other work, and a second ask waits for the
/// first instead of starting git again.
type Log = std::sync::Arc<std::sync::OnceLock<Option<String>>>;
/// A folder and the arguments of a log.
type LogKey = (std::path::PathBuf, Vec<String>);
static LOGS: std::sync::Mutex<Vec<(LogKey, Log)>> = std::sync::Mutex::new(Vec::new());

fn log_cell(dir: &Path, args: &[String]) -> Log {
    let mut all = LOGS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, l)) = all.iter().find(|((d, a), _)| d == dir && a == args) {
        return l.clone();
    }
    let l = Log::default();
    all.push(((dir.to_path_buf(), args.to_vec()), l.clone()));
    l
}

/// `git log` with these arguments, once a run of pg.
fn log_once(dir: &Path, args: Vec<String>) -> Option<String> {
    let cell = log_cell(dir, &args);
    cell.get_or_init(|| git(dir, &args.iter().map(String::as_str).collect::<Vec<_>>())).clone()
}

fn log_ahead(dir: &Path, args: Vec<String>) {
    if !is_repo(dir) {
        return;
    }
    let cell = log_cell(dir, &args);
    if cell.get().is_none() {
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            cell.get_or_init(|| git(&dir, &args.iter().map(String::as_str).collect::<Vec<_>>()));
        });
    }
}

fn since_args(time: i64) -> Vec<String> {
    ["log", &format!("--since=@{time}"), "--name-only", "--format=%x00%s"].map(str::to_string).to_vec()
}

fn together_args(commits: usize) -> Vec<String> {
    ["log", "--name-only", "--format=%x00", "-n", &commits.to_string()].map(str::to_string).to_vec()
}

/// Starts `since` in the background, for a call later in this run.
pub fn since_ahead(dir: &Path, time: i64) {
    log_ahead(dir, since_args(time));
}

/// Starts `changed_together` in the background, for a call later in this run.
pub fn together_ahead(dir: &Path, commits: usize) {
    log_ahead(dir, together_args(commits));
}

/// A file as it was in the last commit; None when it was not there.
pub fn at_head(dir: &Path, path: &str) -> Option<String> {
    git(dir, &["show", &format!("HEAD:./{path}")])
}

pub fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// The changed line ranges of a file; None for a new file (all of it).
pub type Lines = Option<Vec<(usize, usize)>>;

/// What changed in a folder: files with their changed lines.
pub type Changed = Vec<(String, Lines)>;

/// Finding what changed, started early for a `changed` later in this run
/// of pg: git runs while the code index is brought up to date.
static AHEAD: std::sync::Mutex<Vec<(std::path::PathBuf, std::thread::JoinHandle<Changed>)>> =
    std::sync::Mutex::new(Vec::new());

/// Starts finding what changed in these folders. Only where `changed` is
/// sure to follow, and after anything that could change the files.
pub fn changed_ahead<P: AsRef<Path>>(dirs: impl IntoIterator<Item = P>) {
    let Ok(mut ahead) = AHEAD.lock() else { return };
    for d in dirs {
        let d = d.as_ref().to_path_buf();
        if !is_repo(&d) || ahead.iter().any(|(x, _)| *x == d) {
            continue;
        }
        let dir = d.clone();
        ahead.push((d, std::thread::spawn(move || find_changed(&dir))));
    }
}

/// What `changed` found in this run of pg: asked again (the brief, then
/// pg ripple's warnings), git is not run again.
static FOUND: std::sync::Mutex<Vec<(std::path::PathBuf, Changed)>> = std::sync::Mutex::new(Vec::new());

/// Files changed since the last commit, with their changed lines.
pub fn changed(dir: &Path) -> Changed {
    if let Some(known) = FOUND.lock().ok().and_then(|f| f.iter().find(|(d, _)| d == dir).map(|(_, c)| c.clone())) {
        return known;
    }
    let started = AHEAD.lock().ok().and_then(|mut a| a.iter().position(|(x, _)| x == dir).map(|i| a.swap_remove(i).1));
    let found = match started.map(|h| h.join()) {
        Some(Ok(found)) => found,
        _ => find_changed(dir),
    };
    if let Ok(mut f) = FOUND.lock() {
        f.push((dir.to_path_buf(), found.clone()));
    }
    found
}

fn find_changed(dir: &Path) -> Changed {
    if !is_repo(dir) {
        return Vec::new();
    }
    // the two run side by side
    let (diff, untracked) = std::thread::scope(|s| {
        let untracked = s.spawn(|| git(dir, &["ls-files", "--others", "--exclude-standard"]));
        let diff = git(dir, &["diff", "HEAD", "-U0", "--no-color", "--no-ext-diff"]);
        (diff, untracked.join().ok().flatten())
    });
    let mut out: Vec<(String, Lines)> = Vec::new();
    if let Some(diff) = diff {
        let mut file: Option<String> = None;
        for line in diff.lines() {
            if let Some(f) = line.strip_prefix("+++ b/") {
                file = Some(f.to_string());
                out.push((f.to_string(), Some(Vec::new())));
            } else if line.starts_with("+++ /dev/null") {
                file = None; // deleted
            } else if let Some(h) = line.strip_prefix("@@ ")
                && file.is_some()
                && let Some(range) = hunk(h)
                && let Some((_, Some(ranges))) = out.last_mut()
            {
                ranges.push(range);
            }
        }
    }
    if let Some(untracked) = untracked {
        out.extend(untracked.lines().filter(|l| !l.is_empty()).map(|l| (l.to_string(), None)));
    }
    out
}

/// "@@ -10,2 +12,3 @@" -> lines 12 to 14 of the new file.
fn hunk(h: &str) -> Option<(usize, usize)> {
    let new = h.split_whitespace().find(|p| p.starts_with('+'))?.trim_start_matches('+');
    let (start, len): (usize, usize) = match new.split_once(',') {
        Some((s, l)) => (s.parse().ok()?, l.parse().ok()?),
        None => (new.parse().ok()?, 1),
    };
    // a pure deletion still touches the line it happened at
    Some((start.max(1), (start + len.max(1) - 1).max(1)))
}

/// The commit checked out (its first 12 characters).
pub fn head(dir: &Path) -> Option<String> {
    let h = git(dir, &["rev-parse", "HEAD"])?;
    Some(h.trim().chars().take(12).collect())
}

/// The lines of a file (as it is now) changed since a commit, committed or
/// not; None when git does not know the commit or the file.
pub fn changed_since(dir: &Path, commit: &str, path: &str) -> Option<Vec<(usize, usize)>> {
    let diff = git(dir, &["diff", "-U0", "--no-color", "--no-ext-diff", commit, "--", path])?;
    Some(diff.lines().filter_map(|l| l.strip_prefix("@@ ")).filter_map(hunk).collect())
}

/// Commits since a time (seconds since the epoch): their subjects, newest
/// first, and the files they changed.
pub fn since(dir: &Path, time: i64) -> (Vec<String>, Vec<String>) {
    let Some(log) = log_once(dir, since_args(time)) else {
        return (Vec::new(), Vec::new());
    };
    let (mut subjects, mut files) = (Vec::new(), Vec::new());
    for commit in log.split('\0').filter(|c| !c.trim().is_empty()) {
        let mut lines = commit.lines();
        subjects.push(lines.next().unwrap_or("").trim().to_string());
        for f in lines.map(str::trim).filter(|l| !l.is_empty()) {
            if !files.iter().any(|x: &String| x == f) {
                files.push(f.to_string());
            }
        }
    }
    (subjects, files)
}

/// How often each other file changed in the same commit as each file, over
/// the last commits.
pub fn changed_together(dir: &Path, commits: usize) -> HashMap<String, Vec<(String, usize)>> {
    let mut pairs: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let Some(log) = log_once(dir, together_args(commits)) else {
        return HashMap::new();
    };
    for commit in log.split('\0') {
        let files: Vec<&str> = commit.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if files.len() < 2 || files.len() > 30 {
            continue; // one file says nothing; a huge commit is a refactor or an import
        }
        for a in &files {
            for b in &files {
                if a != b {
                    *pairs.entry(a.to_string()).or_default().entry(b.to_string()).or_default() += 1;
                }
            }
        }
    }
    pairs
        .into_iter()
        .map(|(f, others)| {
            let mut v: Vec<(String, usize)> = others.into_iter().filter(|(_, n)| *n >= 2).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            v.truncate(5);
            (f, v)
        })
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn hunks() {
        assert_eq!(super::hunk("-10,2 +12,3 @@ fn x"), Some((12, 14)));
        assert_eq!(super::hunk("-5 +5 @@"), Some((5, 5)));
        assert_eq!(super::hunk("-5,2 +4,0 @@"), Some((4, 4)));
    }
}
