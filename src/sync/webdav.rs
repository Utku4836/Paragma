//! A WebDAV server (Nextcloud, ownCloud, a NAS, many hosts) as a place for
//! the memory: its files listed with PROPFIND, read with GET, written with
//! PUT (folders with MKCOL), removed with DELETE, through curl, as the
//! GitHub calls are. The password is kept by git's credential helper (the
//! system's key store) when there is one, else in pg's own folder, never
//! in the memory.

use super::store::{Listed, Store};
use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct WebDav {
    /// The folder's address, without a / at the end.
    pub url: String,
    pub user: String,
    pub password: String,
    /// Where curl's bodies go meanwhile.
    pub scratch: PathBuf,
}

/// What one call answered.
struct Answer {
    code: u16,
    body: Vec<u8>,
}

impl WebDav {
    pub fn new(url: &str, user: &str, password: &str, scratch: PathBuf) -> WebDav {
        WebDav { url: url.trim_end_matches('/').to_string(), user: user.into(), password: password.into(), scratch }
    }

    fn at(&self, rel: &str) -> String {
        if rel.is_empty() {
            return format!("{}/", self.url);
        }
        let path: Vec<String> = rel.split('/').map(encode).collect();
        format!("{}/{}", self.url, path.join("/"))
    }

    /// One request; the body sent from a file, the answer read from one.
    fn call(&self, method: &str, url: &str, headers: &[&str], body: Option<&[u8]>) -> Result<Answer> {
        crate::sync::fsx::create_dir_all(&self.scratch)?;
        let n = format!("{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')', ' '], "");
        let out = self.scratch.join(format!("dav-out-{n}"));
        let input = self.scratch.join(format!("dav-in-{n}"));
        let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let mut config = format!(
            "url = \"{}\"\nrequest = \"{method}\"\nuser = \"{}:{}\"\nsilent\nshow-error\noutput = \"{}\"\nwrite-out = \"%{{http_code}}\"\n",
            q(url),
            q(&self.user),
            q(&self.password),
            q(&out.to_string_lossy())
        );
        for h in headers {
            config.push_str(&format!("header = \"{}\"\n", q(h)));
        }
        if let Some(b) = body {
            crate::sync::fsx::write(&input, b)?;
            config.push_str(&format!("data-binary = \"@{}\"\n", q(&input.to_string_lossy())));
        }
        let mut child = Command::new("curl")
            .args(["--config", "-", "--max-time", "120", "--retry", "2"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("run curl (is it installed?)")?;
        {
            use std::io::Write;
            child.stdin.take().context("curl's input")?.write_all(config.as_bytes())?;
        }
        let done = child.wait_with_output()?;
        let _ = std::fs::remove_file(&input);
        let body = std::fs::read(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&out);
        let code = String::from_utf8_lossy(&done.stdout).trim().parse::<u16>().unwrap_or(0);
        if code == 0 {
            bail!("{} did not answer: {}", self.place(), String::from_utf8_lossy(&done.stderr).trim());
        }
        if code == 401 {
            bail!("{} refused the user name or password", self.place());
        }
        Ok(Answer { code, body })
    }

    /// The files and folders right under one folder.
    fn propfind(&self, rel: &str, depth: &str) -> Result<Vec<(String, bool, String)>> {
        let body = br#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getetag/><d:getcontentlength/><d:getlastmodified/></d:prop></d:propfind>"#;
        let a = self.call(
            "PROPFIND",
            &self.at(rel),
            &[&format!("Depth: {depth}"), "Content-Type: application/xml"],
            Some(body),
        )?;
        if a.code == 404 {
            return Ok(Vec::new());
        }
        if a.code != 207 {
            bail!("{} answered {} to listing {rel:?}", self.place(), a.code);
        }
        let root = path_of(&self.url);
        Ok(multistatus(&a.body)
            .into_iter()
            .filter_map(|(href, dir, stamp)| {
                let path = decode(&path_of(&href));
                let rest = path.strip_prefix(&root)?.trim_matches('/').to_string();
                Some((rest, dir, stamp))
            })
            .collect())
    }

    /// The folders of a file's path made, from the top.
    fn make_dirs(&self, rel: &str) -> Result<()> {
        let parts: Vec<&str> = rel.split('/').collect();
        for i in 1..parts.len() {
            let dir = parts[..i].join("/");
            let a = self.call("MKCOL", &self.at(&dir), &[], None)?;
            // 405: it is there already
            if !matches!(a.code, 201 | 405 | 301 | 200) {
                bail!("{} answered {} to making the folder {dir}", self.place(), a.code);
            }
        }
        Ok(())
    }

    /// The folders and the files right under `rel`, by their names.
    pub fn entries(&self, rel: &str) -> Result<(Vec<String>, Vec<String>)> {
        let (mut dirs, mut files) = (Vec::new(), Vec::new());
        let at = rel.trim_matches('/');
        for (path, is_dir, _) in self.propfind(at, "1")? {
            let name = match path.strip_prefix(at).map(|r| r.trim_start_matches('/')) {
                Some(n) if !n.is_empty() && !n.contains('/') => n.to_string(),
                _ => continue,
            };
            if is_dir { dirs.push(name) } else { files.push(name) }
        }
        dirs.sort_by_key(|s| s.to_lowercase());
        Ok((dirs, files))
    }

    /// A folder made at `rel` (and those above it).
    pub fn make_folder(&self, rel: &str) -> Result<()> {
        self.make_dirs(&format!("{}/x", rel.trim_matches('/')))
    }

    /// Checks the address and the sign-in: the folder made when missing.
    pub fn check(&self) -> Result<()> {
        let a = self.call("PROPFIND", &self.at(""), &["Depth: 0"], None)?;
        match a.code {
            207 => Ok(()),
            404 => {
                let m = self.call("MKCOL", &self.at(""), &[], None)?;
                if !matches!(m.code, 201 | 405) {
                    bail!("{} answered {} to making the memory's folder", self.place(), m.code);
                }
                Ok(())
            }
            c => bail!("{} answered {c}: is it a WebDAV address?", self.place()),
        }
    }
}

impl Store for WebDav {
    fn list(&self) -> Result<Vec<Listed>> {
        let mut out = Vec::new();
        let mut folders = vec![String::new()];
        // folder by folder (many servers refuse a listing of everything at once)
        while let Some(dir) = folders.pop() {
            for (rel, is_dir, stamp) in self.propfind(&dir, "1")? {
                if rel == dir || rel.is_empty() {
                    continue;
                }
                if is_dir {
                    folders.push(rel);
                } else {
                    out.push(Listed { rel, stamp, settled: true });
                }
            }
        }
        Ok(out)
    }

    fn read(&self, rel: &str) -> Result<Vec<u8>> {
        let a = self.call("GET", &self.at(rel), &[], None)?;
        if a.code != 200 {
            bail!("{} answered {} to reading {rel}", self.place(), a.code);
        }
        Ok(a.body)
    }

    fn write(&self, rel: &str, bytes: &[u8]) -> Result<String> {
        let mut a = self.call("PUT", &self.at(rel), &[], Some(bytes))?;
        if a.code == 404 || a.code == 409 {
            // its folder is not there yet
            self.make_dirs(rel)?;
            a = self.call("PUT", &self.at(rel), &[], Some(bytes))?;
        }
        if !matches!(a.code, 200 | 201 | 204) {
            bail!("{} answered {} to writing {rel}", self.place(), a.code);
        }
        if self.read(rel)? != bytes {
            bail!("{rel} in {} reads back other than written", self.place());
        }
        Ok(self.propfind(rel, "0")?.into_iter().next().map(|(_, _, s)| s).unwrap_or_default())
    }

    fn remove(&self, rel: &str) -> Result<()> {
        let a = self.call("DELETE", &self.at(rel), &[], None)?;
        if !matches!(a.code, 200 | 204 | 404) {
            bail!("{} answered {} to removing {rel}", self.place(), a.code);
        }
        Ok(())
    }

    fn place(&self) -> String {
        self.url.clone()
    }
}

/// The path part of an address ("/remote.php/dav/files/me/Paragma").
fn path_of(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    match rest.find('/') {
        Some(i) if url.contains("://") => rest[i..].to_string(),
        _ if url.starts_with('/') => url.to_string(),
        _ => String::new(),
    }
    .trim_end_matches('/')
    .to_string()
}

/// A path segment as an address carries it.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A multistatus answer: each entry's address, whether it is a folder,
/// and its stamp (ETag, else time and size).
pub fn multistatus(xml: &[u8]) -> Vec<(String, bool, String)> {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_reader(xml);
    let mut out = Vec::new();
    let (mut href, mut dir, mut etag, mut size, mut modified) =
        (String::new(), false, String::new(), String::new(), String::new());
    let mut field = String::new();
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_ascii_lowercase();
                if name == "response" {
                    (href, dir, etag, size, modified) =
                        (String::new(), false, String::new(), String::new(), String::new());
                }
                if name == "collection" {
                    dir = true;
                }
                field = name;
            }
            Ok(Event::Empty(e)) => {
                if e.local_name().as_ref().eq_ignore_ascii_case(b"collection") {
                    dir = true;
                }
            }
            Ok(Event::Text(t)) => {
                let text = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                match field.as_str() {
                    "href" => href.push_str(text.trim()),
                    "getetag" => etag.push_str(text.trim()),
                    "getcontentlength" => size.push_str(text.trim()),
                    "getlastmodified" => modified.push_str(text.trim()),
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                if e.local_name().as_ref().eq_ignore_ascii_case(b"response") && !href.is_empty() {
                    let stamp =
                        if etag.is_empty() { format!("{modified}-{size}") } else { etag.trim_matches('"').to_string() };
                    out.push((href.clone(), dir, stamp));
                }
                field.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

// ---------- the password, kept out of the memory ----------

/// The parts git's credential helper knows an address by.
fn credential_input(url: &str, user: &str, password: Option<&str>) -> String {
    let (protocol, rest) = url.split_once("://").unwrap_or(("https", url));
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let mut s = format!("protocol={protocol}\nhost={host}\npath={path}\nusername={user}\n");
    if let Some(p) = password {
        s.push_str(&format!("password={p}\n"));
    }
    s.push('\n');
    s
}

fn credential(action: &str, input: &str) -> Option<String> {
    let mut child = Command::new("git")
        .args(["-c", "credential.useHttpPath=true", "credential", action])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    {
        use std::io::Write;
        child.stdin.take()?.write_all(input.as_bytes()).ok()?;
    }
    let out = child.wait_with_output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// pg's own place for it, when the system has no key store for git.
fn file(home: &crate::project::Home) -> PathBuf {
    super::dir(home).join("webdav.json")
}

/// Keeps the password: in the system's key store, else in pg's folder.
pub fn save_password(home: &crate::project::Home, url: &str, user: &str, password: &str) -> Result<()> {
    let _ = credential("approve", &credential_input(url, user, Some(password)));
    if password_of(home, url, user).as_deref() == Some(password) {
        return Ok(());
    }
    let v = serde_json::json!({ "url": url, "user": user, "password": password });
    crate::project::files::write_atomic(&file(home), v.to_string().as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(file(home), std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn password_of(home: &crate::project::Home, url: &str, user: &str) -> Option<String> {
    if let Some(out) = credential("fill", &credential_input(url, user, None))
        && let Some(p) = out.lines().find_map(|l| l.strip_prefix("password="))
        && !p.is_empty()
    {
        return Some(p.to_string());
    }
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file(home)).ok()?).ok()?;
    (v["url"].as_str() == Some(url) && v["user"].as_str() == Some(user))
        .then(|| v["password"].as_str().map(str::to_string))
        .flatten()
}

pub fn forget_password(home: &crate::project::Home, url: &str, user: &str) {
    let _ = credential("reject", &credential_input(url, user, None));
    let _ = std::fs::remove_file(file(home));
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/sync/webdav.rs"]
mod tests;
