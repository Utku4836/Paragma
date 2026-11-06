//! GitHub for `pg git`: signing in (the GitHub CLI, or the sign-in git
//! already keeps), listing the repositories, making one, and finding the
//! memory again on a new computer. Calls GitHub's API with curl; the token
//! goes to curl on its input, never on a command line, and is not kept.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// A repository the user can push to.
#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    pub full_name: String,
    pub private: bool,
    pub default_branch: String,
    pub clone_url: String,
    /// Whether it is the user's own (not an organization's).
    pub own: bool,
}

/// Signed in to GitHub.
pub struct Account {
    pub login: String,
    token: String,
    /// The GitHub CLI signed in: its path (git uses it to push).
    pub gh: Option<PathBuf>,
}

/// The GitHub CLI on this computer.
pub fn gh_path() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["gh.exe"] } else { &["gh"] };
    if let Some(paths) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&paths) {
            for n in names {
                let p = d.join(n);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    let usual = if cfg!(windows) {
        vec![PathBuf::from(r"C:\Program Files\GitHub CLI\gh.exe")]
    } else {
        vec![PathBuf::from("/usr/local/bin/gh"), PathBuf::from("/opt/homebrew/bin/gh"), PathBuf::from("/usr/bin/gh")]
    };
    usual.into_iter().find(|p| p.is_file())
}

/// The token of a sign-in this computer already has: the GitHub CLI's,
/// else the one git keeps for github.com (Git Credential Manager, which
/// GitHub Desktop and Git for Windows use). Asks nothing.
fn saved_token(gh: Option<&PathBuf>) -> Option<(String, bool)> {
    if let Some(gh) = gh
        && let Ok(o) =
            Command::new(gh).args(["auth", "token", "--hostname", "github.com"]).stdin(Stdio::null()).output()
        && o.status.success()
    {
        let t = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !t.is_empty() {
            return Some((t, true));
        }
    }
    git_credential(false).map(|t| (t, false))
}

/// git's saved sign-in for github.com; `ask`: let its helper sign in (in
/// the browser) when there is none.
fn git_credential(ask: bool) -> Option<String> {
    let mut c = Command::new("git");
    c.args(["credential", "fill"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    if !ask {
        c.env("GIT_TERMINAL_PROMPT", "0").env("GCM_INTERACTIVE", "never");
    }
    let mut child = c.spawn().ok()?;
    {
        use std::io::Write;
        let mut input = child.stdin.take()?;
        input.write_all(b"protocol=https\nhost=github.com\n\n").ok()?;
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let token = text.lines().find_map(|l| l.strip_prefix("password="))?.trim().to_string();
    if ask && !token.is_empty() {
        // kept for the next time, as git keeps a sign-in that worked
        let mut a = Command::new("git");
        a.args(["credential", "approve"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        if let Ok(mut ch) = a.spawn() {
            use std::io::Write;
            if let Some(mut i) = ch.stdin.take() {
                let _ = i.write_all(text.as_bytes());
                let _ = i.write_all(b"\n");
            }
            let _ = ch.wait();
        }
    }
    (!token.is_empty()).then_some(token)
}

impl Account {
    /// The sign-in this computer already has; None when there is none.
    pub fn saved() -> Result<Option<Account>> {
        let gh = gh_path();
        let Some((token, by_gh)) = saved_token(gh.as_ref()) else { return Ok(None) };
        let mut a = Account { login: String::new(), token, gh: if by_gh { gh } else { None } };
        let (code, me) = a.api("GET", "/user", None)?;
        if code != 200 {
            return Ok(None); // a token GitHub no longer takes
        }
        a.login = me["login"].as_str().unwrap_or("").to_string();
        Ok(Some(a))
    }

    /// Signs in, in the browser with a code: through the GitHub CLI when
    /// it is installed, else through git's helper. The terminal is the
    /// user's meanwhile.
    pub fn sign_in() -> Result<Option<Account>> {
        if let Some(gh) = gh_path() {
            let status = Command::new(&gh)
                .args(["auth", "login", "--hostname", "github.com", "--git-protocol", "https", "--web"])
                .status()
                .context("run the GitHub CLI")?;
            if !status.success() {
                return Ok(None);
            }
            return Account::saved();
        }
        if git_credential(true).is_none() {
            return Ok(None);
        }
        Account::saved()
    }

    /// One call of GitHub's API: its status code and the JSON it answered.
    fn api(&self, method: &str, path: &str, body: Option<Value>) -> Result<(u16, Value)> {
        let mut config = format!(
            "url = \"https://api.github.com{path}\"\nrequest = \"{method}\"\nheader = \"Authorization: Bearer {}\"\n\
             header = \"Accept: application/vnd.github+json\"\nheader = \"X-GitHub-Api-Version: 2022-11-28\"\n\
             header = \"User-Agent: paragma\"\nsilent\nshow-error\nlocation\nwrite-out = \"\\n%{{http_code}}\"\n",
            self.token
        );
        if let Some(b) = body {
            let text = b.to_string().replace('\\', "\\\\").replace('"', "\\\"");
            config.push_str(&format!("header = \"Content-Type: application/json\"\ndata-binary = \"{text}\"\n"));
        }
        let mut child = Command::new("curl")
            .args(["--config", "-", "--max-time", "30"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("run curl (is it installed?)")?;
        {
            use std::io::Write;
            let mut input = child.stdin.take().context("curl's input")?;
            input.write_all(config.as_bytes())?;
        }
        let out = child.wait_with_output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let (body, code) = text.rsplit_once('\n').unwrap_or(("", text.as_ref()));
        let Ok(code) = code.trim().parse::<u16>() else {
            bail!("GitHub did not answer: {}", String::from_utf8_lossy(&out.stderr).trim());
        };
        Ok((code, serde_json::from_str(body).unwrap_or(Value::Null)))
    }

    /// The repositories the user can push to, own and organizations', the
    /// last pushed first.
    pub fn repos(&self) -> Result<Vec<Repo>> {
        let mut out = Vec::new();
        for page in 1..=10 {
            let (code, v) = self.api(
                "GET",
                &format!("/user/repos?per_page=100&page={page}&sort=pushed&affiliation=owner,collaborator,organization_member"),
                None,
            )?;
            if code != 200 {
                bail!("GitHub: {}", v["message"].as_str().unwrap_or("could not list the repositories"));
            }
            let list = v.as_array().cloned().unwrap_or_default();
            let n = list.len();
            out.extend(
                list.iter().filter(|r| r["permissions"]["push"].as_bool().unwrap_or(false)).map(|r| self.repo(r)),
            );
            if n < 100 {
                break;
            }
        }
        Ok(out)
    }

    fn repo(&self, r: &Value) -> Repo {
        let full_name = r["full_name"].as_str().unwrap_or("").to_string();
        Repo {
            own: full_name.split('/').next() == Some(self.login.as_str()),
            full_name,
            private: r["private"].as_bool().unwrap_or(true),
            default_branch: r["default_branch"].as_str().unwrap_or("main").to_string(),
            clone_url: r["clone_url"].as_str().unwrap_or("").to_string(),
        }
    }

    /// Makes a private repository for the memory, marked so it is found
    /// again (`find_memory`).
    pub fn create(&self, name: &str) -> Result<Repo> {
        let body = json!({ "name": name, "private": true, "description": "Paragma: project memory for coding agents", "auto_init": false });
        let (code, v) = self.api("POST", "/user/repos", Some(body))?;
        if code != 201 {
            bail!("GitHub: {}", error_of(&v, "could not make the repository"));
        }
        let repo = self.repo(&v);
        let _ = self.api("PUT", &format!("/repos/{}/topics", repo.full_name), Some(json!({ "names": [super::TOPIC] })));
        Ok(Repo { default_branch: "main".into(), ..repo })
    }

    /// Where a memory already is: the repository, its folder ("" or
    /// Paragma) and branch. By the topic first (one call), then by the
    /// marker in the repositories pushed to last.
    pub fn find_memory(&self, repos: &[Repo]) -> Result<Option<(Repo, String, String)>> {
        let (code, v) = self.api("GET", &format!("/search/repositories?q=topic:{}+fork:true", super::TOPIC), None)?;
        if code == 200
            && let Some(items) = v["items"].as_array()
        {
            for item in items {
                let name = item["full_name"].as_str().unwrap_or("");
                if let Some(r) = repos.iter().find(|r| r.full_name == name)
                    && self.marker_at(r, "", &r.default_branch)
                {
                    return Ok(Some((r.clone(), String::new(), r.default_branch.clone())));
                }
            }
        }
        // the marker, in a few repositories at a time
        let looked: Vec<&Repo> = repos.iter().take(60).collect();
        let found = std::sync::Mutex::new(None);
        std::thread::scope(|s| {
            for chunk in looked.chunks(8) {
                let found = &found;
                s.spawn(move || {
                    for r in chunk {
                        if found.lock().map(|f| f.is_some()).unwrap_or(true) {
                            return;
                        }
                        let at = [
                            (String::new(), r.default_branch.clone()),
                            (super::FOLDER.to_string(), super::BRANCH.to_string()),
                            (super::FOLDER.to_string(), r.default_branch.clone()),
                        ];
                        for (prefix, branch) in at {
                            if self.marker_at(r, &prefix, &branch) {
                                if let Ok(mut f) = found.lock() {
                                    f.get_or_insert(((*r).clone(), prefix, branch));
                                }
                                return;
                            }
                        }
                    }
                });
            }
        });
        Ok(found.into_inner().unwrap_or(None))
    }

    /// Whether the repository is a memory's own: the marker at its root.
    pub fn is_memory(&self, r: &Repo) -> bool {
        self.marker_at(r, "", &r.default_branch)
    }

    fn marker_at(&self, r: &Repo, prefix: &str, branch: &str) -> bool {
        let path = if prefix.is_empty() { super::MARKER.to_string() } else { format!("{prefix}/{}", super::MARKER) };
        matches!(self.api("GET", &format!("/repos/{}/contents/{path}?ref={branch}", r.full_name), None), Ok((200, _)))
    }
}

fn error_of(v: &Value, default: &str) -> String {
    let msg = v["message"].as_str().unwrap_or(default);
    match v["errors"].as_array().and_then(|e| e.first()).and_then(|e| e["message"].as_str()) {
        Some(detail) => format!("{msg}: {detail}"),
        None => msg.to_string(),
    }
}
