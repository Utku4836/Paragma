//! pg's own clone of the memory's repository, and one sync through it:
//! the projects copied in and committed, what other computers sent merged
//! and copied back out, then pushed.

use super::{Config, MARKER, checkout};
use crate::project::Home;
use crate::project::files::{WriteLock, write_atomic};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What a sync did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Files sent: changed here since the last sync.
    pub sent: usize,
    /// Files brought in: changed on other computers.
    pub got: usize,
    pub conflicts: Vec<String>,
    /// The memory moved: the marker's `moved_to` (JSON).
    pub moved_to: Option<String>,
}

impl Outcome {
    pub fn said(&self) -> String {
        match (self.sent, self.got) {
            (0, 0) => "nothing new".to_string(),
            (s, 0) => format!("sent {}", changes(s)),
            (0, g) => format!("brought in {}", changes(g)),
            (s, g) => format!("sent {}, brought in {}", changes(s), changes(g)),
        }
    }
}

fn changes(n: usize) -> String {
    if n == 1 { "1 change".into() } else { format!("{n} changes") }
}

/// Where pg keeps the clone, and what it mirrors.
pub struct Mirror<'a> {
    home: &'a Home,
    config: &'a Config,
    dir: PathBuf,
    /// Whether the .env files go: one setting for every computer, kept in
    /// the marker; the one changed last counts.
    env: std::cell::Cell<bool>,
}

/// A file's bytes as one number: whether it changed.
fn fingerprint(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl<'a> Mirror<'a> {
    /// The clone, made when it is not there yet (or is of another remote).
    pub fn new(home: &'a Home, config: &'a Config) -> Result<Mirror<'a>> {
        let m = Mirror { home, config, dir: checkout(home), env: std::cell::Cell::new(config.env && !config.public) };
        let ours = m.dir.join(".git").is_dir()
            && m.git(&["config", "--get", "remote.origin.url"]).map(|u| u.trim() == config.remote).unwrap_or(false);
        if !ours {
            super::remove_clone(&m.dir)?;
            m.setup()?;
        }
        m.identity()?;
        Ok(m)
    }

    /// The folder in the clone that holds the projects.
    fn root(&self) -> PathBuf {
        if self.config.prefix.is_empty() { self.dir.clone() } else { self.dir.join(&self.config.prefix) }
    }

    fn setup(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let c = self.config;
        self.git(&["init", "-q"])?;
        for (k, v) in [
            // the files are copied byte for byte: git changes nothing in them
            ("core.autocrlf", "false"),
            ("core.safecrlf", "false"),
            ("core.longpaths", "true"),
            ("core.quotepath", "off"),
        ] {
            self.git(&["config", k, v])?;
        }
        self.git(&["remote", "add", "origin", &c.remote])?;
        // a repository that holds other things: only the memory's folder is
        // fetched and checked out
        let partial = !c.prefix.is_empty() && c.branch != super::BRANCH;
        if partial {
            if c.remote.starts_with("https://") {
                self.git(&["config", "remote.origin.promisor", "true"])?;
                self.git(&["config", "remote.origin.partialclonefilter", "blob:none"])?;
            }
            self.git(&["sparse-checkout", "set", "--no-cone", &format!("/{}/", c.prefix)])?;
        }
        // both sides' lines are kept when two computers changed one note
        let info = self.dir.join(".git/info");
        std::fs::create_dir_all(&info)?;
        std::fs::write(info.join("attributes"), "*.md merge=union\n*.env merge=union\n")?;
        if self.fetch()? {
            self.git(&["checkout", "-q", "-B", &c.branch, &format!("origin/{}", c.branch)])?;
        } else {
            self.git(&["checkout", "-q", "--orphan", &c.branch])?;
        }
        Ok(())
    }

    /// Who commits, kept in the clone itself: the user's own git identity
    /// may be set or removed later, a commit must not fail on it.
    fn identity(&self) -> Result<()> {
        let local =
            |k: &str| self.git(&["config", "--local", k]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if local("user.email").is_some() && local("user.name").is_some() {
            return Ok(());
        }
        let any = |k: &str| self.git(&["config", k]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let c = self.config;
        let name = any("user.name")
            .unwrap_or_else(|| if c.account.is_empty() { "Paragma".to_string() } else { c.account.clone() });
        let email = any("user.email").unwrap_or_else(|| {
            if c.account.is_empty() {
                "paragma@localhost".to_string()
            } else {
                format!("{}@users.noreply.github.com", c.account)
            }
        });
        self.git(&["config", "--local", "user.name", &name])?;
        self.git(&["config", "--local", "user.email", &email])?;
        Ok(())
    }

    /// Runs git in the clone, signing in without asking anyone.
    pub fn git(&self, args: &[&str]) -> Result<String> {
        let out = self.git_output(args)?;
        if !out.status.success() {
            bail!("git {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn git_output(&self, args: &[&str]) -> Result<std::process::Output> {
        let mut c = Command::new("git");
        if !self.config.gh.is_empty() {
            let helper = format!("credential.helper=!'{}' auth git-credential", self.config.gh.replace('\\', "/"));
            c.args(["-c", "credential.helper=", "-c", &helper]);
        }
        c.args(args)
            .current_dir(&self.dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(std::process::Stdio::null());
        c.output().context("run git (is it installed?)")
    }

    /// Fetches the memory's branch; false when the remote has none yet.
    fn fetch(&self) -> Result<bool> {
        let c = self.config;
        let refspec = format!("+refs/heads/{0}:refs/remotes/origin/{0}", c.branch);
        let partial = !c.prefix.is_empty() && c.branch != super::BRANCH && c.remote.starts_with("https://");
        let mut args = vec!["fetch", "-q", "--no-tags"];
        if partial {
            args.push("--filter=blob:none");
        }
        args.extend(["origin", refspec.as_str()]);
        let out = self.git_output(&args)?;
        if out.status.success() {
            return Ok(true);
        }
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("couldn't find remote ref") || err.contains("could not find remote ref") {
            return Ok(false);
        }
        bail!("git fetch: {}", err.trim())
    }

    fn head(&self) -> Option<String> {
        self.git(&["rev-parse", "-q", "--verify", "HEAD"]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    /// Whether the repository already holds a Paragma memory.
    pub fn has_memory(&self) -> bool {
        self.root().join(MARKER).exists()
    }

    /// With a Paragma/ folder: whether the repository is a memory's own
    /// already (the marker at its root), where a folder would look like a
    /// project.
    pub fn memory_at_root(&self) -> bool {
        !self.config.prefix.is_empty()
            && self.git(&["cat-file", "-e", &format!("origin/{}:{MARKER}", self.config.branch)]).is_ok()
    }

    /// The files of the projects that go, by their path under projects/.
    fn local_files(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        walk(&self.home.projects_dir(), "", &mut out, &|rel, is_dir| kept(rel, is_dir, self.env.get()))?;
        Ok(out)
    }

    /// The files in the clone's memory folder, by the same paths.
    fn clone_files(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let root = self.root();
        if root.is_dir() {
            walk(&root, "", &mut out, &|rel, is_dir| rel != ".git" && rel != MARKER && kept(rel, is_dir, true))?;
        }
        Ok(out)
    }

    /// Copies the projects into the clone, removing there what is gone here.
    /// Returns what each file was, to see later whether it changed here.
    fn copy_in(&self, remove: bool) -> Result<HashMap<String, u64>> {
        let projects = self.home.projects_dir();
        let root = self.root();
        let mut seen = HashMap::new();
        for rel in self.local_files()? {
            let Ok(bytes) = std::fs::read(projects.join(&rel)) else { continue };
            seen.insert(rel.clone(), fingerprint(&bytes));
            let to = root.join(&rel);
            if std::fs::read(&to).ok().as_deref() != Some(bytes.as_slice()) {
                if let Some(d) = to.parent() {
                    std::fs::create_dir_all(d)?;
                }
                std::fs::write(&to, &bytes)?;
            }
        }
        if remove {
            for rel in self.clone_files()? {
                if !seen.contains_key(&rel) {
                    std::fs::remove_file(root.join(&rel))?;
                    remove_empty_dirs(&root, &rel);
                }
            }
        }
        Ok(seen)
    }

    /// Commits what is staged and changed; how many files.
    fn commit(&self, message: &str) -> Result<usize> {
        let path = if self.config.prefix.is_empty() { ".".to_string() } else { self.config.prefix.clone() };
        self.git(&["add", "-A", "--", &path])?;
        let names = self.git(&["diff", "--cached", "--name-only", "--no-renames"])?;
        let n = names.lines().filter(|l| !l.ends_with(MARKER)).count();
        if names.trim().is_empty() {
            return Ok(0);
        }
        self.git(&["commit", "-q", "-m", message])?;
        Ok(n)
    }

    /// Writes the marker: the memory is here, and how.
    fn mark(&self, moved_to: Option<Value>) -> Result<()> {
        let mut v = json!({ "paragma": 1, "env": self.env.get(), "env_at": self.config.env_at });
        if let Some(m) = moved_to {
            v["moved_to"] = m;
        }
        let root = self.root();
        std::fs::create_dir_all(&root)?;
        std::fs::write(root.join(MARKER), serde_json::to_string_pretty(&v)? + "\n")?;
        Ok(())
    }

    fn marker(&self) -> Option<Value> {
        serde_json::from_str(&std::fs::read_to_string(self.root().join(MARKER)).ok()?).ok()
    }

    /// The .env setting: the marker's when it was changed later than this
    /// computer's (kept here too), else this computer's (into the marker).
    fn settle_env(&self) -> Result<()> {
        let Some(m) = self.marker() else { return Ok(()) };
        let (theirs, at) = (m["env"].as_bool().unwrap_or(false), m["env_at"].as_i64().unwrap_or(0));
        if at > self.config.env_at {
            let env = theirs && !self.config.public;
            self.env.set(env);
            let mut c = self.config.clone();
            c.env = env;
            c.env_at = at;
            c.save(self.home)?;
        } else if theirs != self.env.get() || at < self.config.env_at {
            let moved = m.get("moved_to").cloned().filter(|v| !v.is_null());
            self.mark(moved)?;
        }
        Ok(())
    }

    /// One sync: send, bring in, push.
    pub fn sync(&self) -> Result<Outcome> {
        let mut out = Outcome::default();
        let host = host();
        if !self.has_memory() {
            self.mark(None)?;
        }
        self.settle_env()?;
        let seen = self.copy_in(true)?;
        out.sent = self.commit(&format!("Paragma: {host}"))?;
        if self.fetch()? {
            let theirs = format!("origin/{}", self.config.branch);
            let before = self.head();
            let behind = match &before {
                Some(h) => self.git(&["rev-list", "--count", &format!("{h}..{theirs}")])?.trim() != "0",
                None => true,
            };
            if behind {
                match &before {
                    Some(_) => out.conflicts = self.merge(&theirs)?,
                    None => {
                        self.git(&["reset", "-q", "--hard", &theirs])?;
                    }
                }
                if let Some(to) = self.marker().and_then(|m| m.get("moved_to").cloned()).filter(|m| !m.is_null()) {
                    out.moved_to = Some(to.to_string());
                    return Ok(out);
                }
                // turned on (or off) on another computer
                let had = self.env.get();
                self.settle_env()?;
                if self.env.get() && !had {
                    // the .env files that were there before count as new here
                    out.got += self.copy_out(None, &seen)?;
                }
                out.got += self.copy_out(before.as_deref(), &seen)?;
            }
        }
        self.push()?;
        Ok(out)
    }

    /// Merges the other computers' commits; the files both sides changed
    /// that git could not join are kept as they are here, theirs beside.
    fn merge(&self, theirs: &str) -> Result<Vec<String>> {
        let out = self.git_output(&["merge", "-q", "--no-edit", "-m", "Paragma: merge", theirs])?;
        if out.status.success() {
            return Ok(Vec::new());
        }
        let both = self.git(&["diff", "--name-only", "--diff-filter=U"])?;
        if both.trim().is_empty() {
            bail!("git merge: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        let mut kept = Vec::new();
        for path in both.lines().filter(|l| !l.is_empty()) {
            if let Ok(theirs) = self.git(&["show", &format!(":3:{path}")]) {
                let side = conflict_name(path);
                std::fs::write(self.dir.join(&side), theirs)?;
                self.git(&["add", "--", &side])?;
                kept.push(side);
            }
            let _ = self.git(&["checkout", "-q", "--ours", "--", path]);
            self.git(&["add", "--", path])?;
        }
        self.git(&["commit", "-q", "--no-edit", "-m", "Paragma: merge"])?;
        Ok(kept)
    }

    /// Copies what other computers changed into the projects: each file
    /// only when it did not change here since it was copied in (it goes in
    /// the next sync then). `before`: the commit before the merge, None
    /// when everything is new here. How many files.
    fn copy_out(&self, before: Option<&str>, seen: &HashMap<String, u64>) -> Result<usize> {
        let root_path = if self.config.prefix.is_empty() { String::new() } else { format!("{}/", self.config.prefix) };
        let mut changed: Vec<(String, bool)> = Vec::new(); // project path, gone
        match before {
            Some(b) => {
                let list = self.git(&["diff", "--name-status", "--no-renames", "-z", b, "HEAD"])?;
                let mut parts = list.split('\0').filter(|s| !s.is_empty());
                while let (Some(st), Some(path)) = (parts.next(), parts.next()) {
                    let Some(rel) = path.strip_prefix(&root_path) else { continue };
                    if rel == MARKER || !kept(rel, false, self.env.get()) {
                        continue;
                    }
                    changed.push((rel.to_string(), st.starts_with('D')));
                }
            }
            None => changed.extend(self.clone_files()?.into_iter().map(|r| (r, false))),
        }
        let projects = self.home.projects_dir();
        let root = self.root();
        let mut by_project: HashMap<String, Vec<(String, bool)>> = HashMap::new();
        for (rel, gone) in changed {
            let p = rel.split('/').next().unwrap_or("").to_string();
            by_project.entry(p).or_default().push((rel, gone));
        }
        let mut n = 0;
        for (project, files) in by_project {
            // pg's own writers in that project wait meanwhile
            let _lock = if project.starts_with('.') || project.is_empty() {
                None
            } else {
                Some(WriteLock::take(&projects.join(&project).join(".pg"))?)
            };
            for (rel, gone) in files {
                let here = projects.join(&rel);
                let now = std::fs::read(&here).ok().map(|b| fingerprint(&b));
                if now != seen.get(&rel).copied() {
                    continue; // changed here meanwhile: it goes next time
                }
                if gone {
                    if here.exists() {
                        std::fs::remove_file(&here)?;
                        remove_empty_dirs(&projects, &rel);
                        n += 1;
                    }
                } else if let Ok(bytes) = std::fs::read(root.join(&rel))
                    && now != Some(fingerprint(&bytes))
                {
                    write_atomic(&here, &bytes)?;
                    n += 1;
                }
            }
        }
        Ok(n)
    }

    fn push(&self) -> Result<()> {
        let Some(head) = self.head() else { return Ok(()) };
        let theirs = format!("origin/{}", self.config.branch);
        let known = self.git(&["rev-parse", "-q", "--verify", &theirs]).map(|s| s.trim().to_string()).ok();
        if known.as_deref() == Some(head.as_str()) {
            return Ok(());
        }
        let target = format!("HEAD:refs/heads/{}", self.config.branch);
        for _ in 0..3 {
            let out = self.git_output(&["push", "-q", "origin", &target])?;
            if out.status.success() {
                let _ = self.git(&["update-ref", &format!("refs/remotes/{theirs}"), "HEAD"]);
                return Ok(());
            }
            let err = String::from_utf8_lossy(&out.stderr).to_string();
            if !(err.contains("rejected") || err.contains("fetch first") || err.contains("non-fast-forward")) {
                bail!("git push: {}", err.trim());
            }
            // another computer pushed meanwhile: take it in and try again
            if self.fetch()? {
                let seen: HashMap<String, u64> = self
                    .local_files()?
                    .into_iter()
                    .filter_map(|r| std::fs::read(self.home.projects_dir().join(&r)).ok().map(|b| (r, fingerprint(&b))))
                    .collect();
                let before = self.head();
                self.merge(&theirs)?;
                self.copy_out(before.as_deref(), &seen)?;
            }
        }
        bail!("git push: the repository kept changing; it is tried again on the next sync")
    }

    /// Connecting to a repository that already holds a memory: what is
    /// here and what is there are joined (a note both have, with both
    /// sides' lines), committed, and the result copied into the projects.
    pub fn restore(&self) -> Result<Outcome> {
        let mut out = Outcome::default();
        self.settle_env()?;
        let projects = self.home.projects_dir();
        let root = self.root();
        let mut joined = 0;
        for rel in self.local_files()? {
            let Ok(here) = std::fs::read(projects.join(&rel)) else { continue };
            let there_path = root.join(&rel);
            match std::fs::read(&there_path) {
                Ok(there) if there == here => {}
                Ok(there) => {
                    std::fs::write(&there_path, union(&here, &there, &self.dir)?)?;
                    joined += 1;
                }
                Err(_) => {
                    if let Some(d) = there_path.parent() {
                        std::fs::create_dir_all(d)?;
                    }
                    std::fs::write(&there_path, &here)?;
                    joined += 1;
                }
            }
        }
        if joined > 0 {
            self.mark(None)?;
            out.sent = self.commit(&format!("Paragma: {} joined its memory", host()))?;
        }
        // everything there, into the projects
        for rel in self.clone_files()?.into_iter().filter(|r| kept(r, false, self.env.get())) {
            let Ok(bytes) = std::fs::read(root.join(&rel)) else { continue };
            let here = projects.join(&rel);
            if std::fs::read(&here).ok().as_deref() != Some(bytes.as_slice()) {
                let project = rel.split('/').next().unwrap_or("");
                let _lock = if project.starts_with('.') {
                    None
                } else {
                    Some(WriteLock::take(&projects.join(project).join(".pg"))?)
                };
                write_atomic(&here, &bytes)?;
                out.got += 1;
            }
        }
        self.push()?;
        Ok(out)
    }

    /// Leaves a note in this repository that the memory moved, so other
    /// computers follow; with `remove`, the memory's files go from it (they
    /// stay in its history).
    pub fn moved(&self, to: &Config, remove: bool) -> Result<()> {
        let _ = self.fetch().and_then(|found| {
            if found && self.head().is_some() {
                self.merge(&format!("origin/{}", self.config.branch)).map(|_| ())
            } else {
                Ok(())
            }
        });
        if remove {
            for rel in self.clone_files()? {
                std::fs::remove_file(self.root().join(&rel))?;
                remove_empty_dirs(&self.root(), &rel);
            }
        }
        let place = json!({ "repo": to.repo, "remote": to.remote, "prefix": to.prefix, "branch": to.branch });
        self.mark(Some(place))?;
        self.commit(&format!("Paragma: the memory moved to {}", to.place()))?;
        self.push()
    }
}

/// Reads where the memory moved, from a marker's `moved_to`.
pub fn parse_moved(to: &str) -> Option<(String, String, String, String)> {
    let v: Value = serde_json::from_str(to).ok()?;
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    let remote = s("remote");
    (!remote.is_empty()).then(|| (s("repo"), remote, s("prefix"), s("branch")))
}

/// Both versions of a note, lines of each kept (git's union merge, from an
/// empty base).
fn union(here: &[u8], there: &[u8], scratch: &Path) -> Result<Vec<u8>> {
    let dir = scratch.join(".git").join("paragma-join");
    std::fs::create_dir_all(&dir)?;
    let (a, base, b) = (dir.join("here"), dir.join("base"), dir.join("there"));
    std::fs::write(&a, here)?;
    std::fs::write(&base, b"")?;
    std::fs::write(&b, there)?;
    let out = Command::new("git")
        .args(["merge-file", "-p", "--union"])
        .args([&a, &base, &b])
        .output()
        .context("run git merge-file")?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(out.stdout)
}

/// The name a file's other version is kept under: notes.conflict.md.
fn conflict_name(path: &str) -> String {
    match path.rsplit_once('.') {
        Some((stem, ext)) if !stem.ends_with('/') && !stem.is_empty() => format!("{stem}.conflict.{ext}"),
        _ => format!("{path}.conflict"),
    }
}

/// What goes into the repository, by its path under projects/: not the
/// indexes, state and lock (.pg/), not this computer's folder links, not
/// files being written, the .env files only when chosen.
fn kept(rel: &str, is_dir: bool, env: bool) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    if is_dir {
        return name != ".pg" && name != ".git";
    }
    if rel == ".links" || name.ends_with(".tmp") {
        return false;
    }
    env || !name.ends_with(".env")
}

/// Files below `root`, by their path with '/', as `keep` allows.
fn walk(root: &Path, rel: &str, out: &mut Vec<String>, keep: &dyn Fn(&str, bool) -> bool) -> Result<()> {
    let dir = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(()) };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            if keep(&path, true) {
                walk(root, &path, out, keep)?;
            }
        } else if t.is_file() && keep(&path, false) {
            out.push(path);
        }
    }
    Ok(())
}

/// Removes the folders of `rel` that became empty, up to `root`.
fn remove_empty_dirs(root: &Path, rel: &str) {
    let mut p = PathBuf::from(rel);
    while p.pop() && !p.as_os_str().is_empty() {
        let d = root.join(&p);
        let only_pg = std::fs::read_dir(&d).map(|es| es.flatten().all(|e| e.file_name() == ".pg")).unwrap_or(false);
        // a project left with nothing but its indexes goes with them
        if only_pg && p.components().count() == 1 && d.join(".pg").exists() {
            let _ = std::fs::remove_dir_all(&d);
        } else if std::fs::remove_dir(&d).is_err() {
            break;
        }
    }
}

/// This computer's name, for commit messages.
fn host() -> String {
    for k in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(h) = std::env::var(k)
            && !h.trim().is_empty()
        {
            return h.trim().to_string();
        }
    }
    std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).unwrap_or_else(|_| "a computer".into())
}
