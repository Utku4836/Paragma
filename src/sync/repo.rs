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
pub(crate) fn fingerprint(bytes: &[u8]) -> u64 {
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
        Mirror::at(home, config, checkout(home))
    }

    /// A clone in another folder (the place the memory leaves, while the
    /// new one is pg's usual clone).
    pub fn at(home: &'a Home, config: &'a Config, dir: PathBuf) -> Result<Mirror<'a>> {
        let m = Mirror { home, config, dir, env: std::cell::Cell::new(config.env && !config.public) };
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
        crate::sync::fsx::create_dir_all(&self.dir)?;
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
        crate::sync::fsx::create_dir_all(&info)?;
        crate::sync::fsx::write(info.join("attributes"), "*.md merge=union\n*.env merge=union\n")?;
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
        let held = self.held();
        let mut out = Vec::new();
        walk(&self.home.projects_dir(), "", &mut out, &|rel, is_dir| {
            kept(rel, is_dir, self.env.get()) && (is_dir || !held.contains(rel)) && self.config.goes(rel)
        })?;
        Ok(out)
    }

    /// The files kept with notes that stay on this computer: all of them
    /// when they are not sent, else those over the size limit. Neither sent
    /// nor taken out of the repository: another computer's copy stays.
    fn held(&self) -> std::collections::HashSet<String> {
        let send = self.config.attachments && !self.config.public;
        let most = self.config.attach_mb * 1024 * 1024;
        crate::project::attach::all(&self.home.projects_dir())
            .into_iter()
            .filter(|(_, size)| !send || *size > most)
            .map(|(rel, _)| rel)
            .collect()
    }

    /// The files in the clone's memory folder, by the same paths.
    fn clone_files(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let root = self.root();
        if root.is_dir() {
            walk(&root, "", &mut out, &|rel, is_dir| {
                rel != ".git" && rel != MARKER && kept(rel, is_dir, true) && self.config.goes(rel)
            })?;
        }
        Ok(out)
    }

    /// Copies the projects into the clone, removing there what is gone here.
    /// Returns what each file was, to see later whether it changed here.
    fn copy_in(&self, remove: bool) -> Result<HashMap<String, u64>> {
        let projects = self.home.projects_dir();
        let root = self.root();
        let mut seen = HashMap::new();
        let local = self.local_files()?;
        let total = local.len();
        for (i, rel) in local.into_iter().enumerate() {
            super::progress::set(self.home, "copying", i + 1, total);
            let Ok(bytes) = std::fs::read(projects.join(&rel)) else { continue };
            seen.insert(rel.clone(), fingerprint(&bytes));
            let to = root.join(&rel);
            if std::fs::read(&to).ok().as_deref() != Some(bytes.as_slice()) {
                if let Some(d) = to.parent() {
                    crate::sync::fsx::create_dir_all(d)?;
                }
                crate::sync::fsx::write(&to, &bytes)?;
            }
        }
        if remove {
            let held = self.held();
            for rel in self.clone_files()? {
                if !seen.contains_key(&rel) && !held.contains(&rel) {
                    crate::sync::fsx::remove_file(root.join(&rel))?;
                    remove_empty_dirs(&root, &rel);
                }
            }
        }
        Ok(seen)
    }

    /// Commits what is staged and changed; how many files.
    fn commit(&self, message: &str) -> Result<usize> {
        let path = if self.config.prefix.is_empty() { ".".to_string() } else { self.config.prefix.clone() };
        // forced: a .gitignore inside the memory (a project's own) never
        // keeps one of its files out
        self.git(&["add", "-A", "-f", "--", &path])?;
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
        let mut v = json!({ "paragma": 1, "id": self.config.id, "env": self.env.get(), "env_at": self.config.env_at });
        if let Some(m) = moved_to {
            v["moved_to"] = m;
        }
        let root = self.root();
        crate::sync::fsx::create_dir_all(&root)?;
        crate::sync::fsx::write(root.join(MARKER), serde_json::to_string_pretty(&v)? + "\n")?;
        Ok(())
    }

    pub fn marker(&self) -> Option<Value> {
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
                crate::sync::fsx::write(self.dir.join(&side), theirs)?;
                self.git(&["add", "-f", "--", &side])?;
                kept.push(side);
            }
            let _ = self.git(&["checkout", "-q", "--ours", "--", path]);
            self.git(&["add", "-f", "--", path])?;
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
                    if rel == MARKER || !kept(rel, false, self.env.get()) || !self.config.goes(rel) {
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
                        crate::sync::fsx::remove_gone(&here)?;
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
        super::progress::set(self.home, "sending to the repository", 0, 0);
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
                    crate::sync::fsx::write(&there_path, union(&here, &there, &self.dir)?)?;
                    joined += 1;
                }
                Err(_) => {
                    if let Some(d) = there_path.parent() {
                        crate::sync::fsx::create_dir_all(d)?;
                    }
                    crate::sync::fsx::write(&there_path, &here)?;
                    joined += 1;
                }
            }
        }
        // connected to on purpose: a note that the memory left is taken out
        let noted = self.marker().is_some_and(|m| m.get("moved_to").is_some_and(|v| !v.is_null()));
        if joined > 0 || noted {
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

    // ---------- a memory found there: replaced, taken, removed ----------

    /// The clone made what the repository holds now.
    fn as_there(&self) -> Result<()> {
        if self.fetch()? {
            let theirs = format!("origin/{}", self.config.branch);
            self.git(&["checkout", "-q", "-B", &self.config.branch, &theirs])?;
            self.git(&["reset", "-q", "--hard", &theirs])?;
        }
        Ok(())
    }

    /// The memory there, whole: each file's path and bytes.
    pub fn read_all(&self) -> Result<Vec<(String, Vec<u8>)>> {
        self.as_there()?;
        let root = self.root();
        let mut out = Vec::new();
        for rel in self.clone_files()? {
            out.push((rel.clone(), crate::sync::fsx::read(root.join(&rel))?));
        }
        Ok(out)
    }

    /// The repository made what this computer's memory is (what was there
    /// only taken out; it stays in the history), then synced as one.
    pub fn replace(&self) -> Result<Outcome> {
        self.as_there()?;
        self.copy_in(true)?;
        self.mark(None)?;
        let sent = self.commit(&format!("Paragma: {} put its memory in place of this one", host()))?;
        self.push()?;
        Ok(Outcome { sent, ..Outcome::default() })
    }

    /// This computer's memory made what the repository holds.
    pub fn take(&self) -> Result<Outcome> {
        let files = self.read_all()?;
        let got = take_files(self.home, &files, &self.local_files()?)?;
        let mut out = self.restore()?;
        out.got += got;
        Ok(out)
    }

    /// The memory's files and its marker taken out of the repository (they
    /// stay in its history).
    pub fn remove_all(&self) -> Result<()> {
        self.as_there()?;
        let root = self.root();
        for rel in self.clone_files()? {
            crate::sync::fsx::remove_gone(root.join(&rel))?;
            remove_empty_dirs(&root, &rel);
        }
        crate::sync::fsx::remove_gone(root.join(MARKER))?;
        self.commit("Paragma: the memory taken out")?;
        self.push()
    }

    /// The connection works: the repository's branch is what was pushed.
    pub fn probe(&self) -> Result<()> {
        let head = self.head().context("nothing was committed")?;
        let out = self.git(&["ls-remote", "origin", &format!("refs/heads/{}", self.config.branch)])?;
        let there = out.split_whitespace().next().unwrap_or("");
        if there != head {
            bail!("the repository's branch {} is not what was sent", self.config.branch);
        }
        Ok(())
    }

    /// Every file that goes is in the clone as sent, byte for byte: what
    /// differs.
    pub fn verify(&self) -> Result<Vec<String>> {
        let projects = self.home.projects_dir();
        let root = self.root();
        let mut wrong = Vec::new();
        // what git holds, not only what is in the clone's folder
        let tracked: std::collections::HashSet<String> = self
            .git(&["-c", "core.quotepath=off", "ls-files", "-z"])?
            .split('\0')
            .filter(|l| !l.is_empty())
            .map(|l| match self.config.prefix.as_str() {
                "" => l.to_string(),
                p => l.strip_prefix(&format!("{p}/")).unwrap_or(l).to_string(),
            })
            .collect();
        let local = self.local_files()?;
        let total = local.len();
        for (i, rel) in local.into_iter().enumerate() {
            super::progress::set(self.home, "checking", i + 1, total);
            // a file taken out here meanwhile is not one that went wrong
            let Ok(here) = std::fs::read(projects.join(&rel)) else { continue };
            if !tracked.contains(&rel) || std::fs::read(root.join(&rel)).ok().as_deref() != Some(here.as_slice()) {
                wrong.push(rel);
            }
        }
        // and the clone holds no change that was not sent
        if !self.git(&["status", "--porcelain"])?.trim().is_empty() {
            wrong.push("(the clone has changes not sent)".into());
        }
        Ok(wrong)
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
                crate::sync::fsx::remove_file(self.root().join(&rel))?;
                remove_empty_dirs(&self.root(), &rel);
            }
        }
        self.mark(Some(to.place_json()))?;
        self.commit(&format!("Paragma: the memory moved to {}", to.place()))?;
        self.push()
    }
}

/// Both versions of a note, lines of each kept (git's union merge, from an
/// empty base).
pub(crate) fn union(here: &[u8], there: &[u8], scratch: &Path) -> Result<Vec<u8>> {
    let dir = scratch.join(".git").join("paragma-join");
    crate::sync::fsx::create_dir_all(&dir)?;
    let (a, base, b) = (dir.join("here"), dir.join("base"), dir.join("there"));
    crate::sync::fsx::write(&a, here)?;
    crate::sync::fsx::write(&base, b"")?;
    crate::sync::fsx::write(&b, there)?;
    let out = Command::new("git")
        .args(["merge-file", "-p", "--union"])
        .args([&a, &base, &b])
        .output()
        .context("run git merge-file")?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(out.stdout)
}

/// The name a file's other version is kept under: notes.conflict.md.
pub(crate) fn conflict_name(path: &str) -> String {
    match path.rsplit_once('.') {
        Some((stem, ext)) if !stem.ends_with('/') && !stem.is_empty() => format!("{stem}.conflict.{ext}"),
        _ => format!("{path}.conflict"),
    }
}

/// What goes into the repository, by its path under projects/: not the
/// indexes, state and lock (.pg/), not this computer's folder links, not
/// files being written, the .env files only when chosen.
pub(crate) fn kept(rel: &str, is_dir: bool, env: bool) -> bool {
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
pub(crate) fn walk(root: &Path, rel: &str, out: &mut Vec<String>, keep: &dyn Fn(&str, bool) -> bool) -> Result<()> {
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

/// This computer's memory made what `files` hold: each one written, and
/// of `local` (the files that go from here) those not among them taken
/// out. How many changed here.
pub(crate) fn take_files(home: &Home, files: &[(String, Vec<u8>)], local: &[String]) -> Result<usize> {
    let projects = home.projects_dir();
    let names: std::collections::HashSet<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    let mut n = 0;
    for (rel, bytes) in files {
        let here = projects.join(rel);
        if std::fs::read(&here).ok().as_deref() == Some(bytes.as_slice()) {
            continue;
        }
        let project = rel.split('/').next().unwrap_or("");
        let _lock = if project.starts_with('.') || !rel.contains('/') {
            None
        } else {
            let meta = projects.join(project).join(".pg");
            crate::sync::fsx::create_dir_all(&meta)?;
            Some(WriteLock::take(&meta)?)
        };
        write_atomic(&here, bytes)?;
        n += 1;
    }
    for rel in local {
        if !names.contains(rel.as_str()) {
            crate::sync::fsx::remove_gone(projects.join(rel))?;
            remove_empty_dirs(&projects, rel);
            n += 1;
        }
    }
    Ok(n)
}

/// Removes the folders of `rel` that became empty, up to `root`.
pub(crate) fn remove_empty_dirs(root: &Path, rel: &str) {
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
pub(crate) fn host() -> String {
    for k in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(h) = std::env::var(k)
            && !h.trim().is_empty()
        {
            return h.trim().to_string();
        }
    }
    std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).unwrap_or_else(|_| "a computer".into())
}
