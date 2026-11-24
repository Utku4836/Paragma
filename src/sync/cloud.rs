//! The cloud folders on this computer: where OneDrive, Google Drive,
//! Dropbox and iCloud keep their files here. A memory in one is kept by the
//! cloud and its path below the cloud's own folder, so another computer,
//! whose cloud folder is somewhere else, finds it in its own; and by the
//! memory's id, when that path is not there (a folder named in another
//! language, moved by hand).

use super::MARKER;
use std::path::{Path, PathBuf};

/// A cloud with a folder on this computer.
#[derive(Clone, Debug, PartialEq)]
pub struct Cloud {
    pub id: &'static str,
    pub name: &'static str,
    pub root: PathBuf,
}

pub const CLOUDS: [(&str, &str); 4] =
    [("onedrive", "OneDrive"), ("gdrive", "Google Drive"), ("dropbox", "Dropbox"), ("icloud", "iCloud Drive")];

pub fn name(id: &str) -> &'static str {
    CLOUDS.iter().find(|c| c.0 == id).map(|c| c.1).unwrap_or("a folder")
}

fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_default()
}

/// `PARAGMA_CLOUDS="onedrive=C:\x;gdrive=D:\y"` names the clouds' folders
/// instead of finding them (tests, an unusual setup).
fn named() -> Option<Vec<(String, PathBuf)>> {
    let v = std::env::var("PARAGMA_CLOUDS").ok()?;
    Some(
        v.split(';')
            .filter_map(|p| p.split_once('='))
            .map(|(k, p)| (k.trim().to_string(), PathBuf::from(p.trim())))
            .collect(),
    )
}

/// The clouds that have a folder here.
pub fn found() -> Vec<Cloud> {
    CLOUDS.iter().filter_map(|(id, name)| root(id).map(|root| Cloud { id, name, root })).collect()
}

/// What this computer has of a cloud: its program, whether it runs, the
/// account signed in to it, and its folder here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub id: &'static str,
    pub name: &'static str,
    pub root: Option<PathBuf>,
    pub installed: bool,
    pub running: bool,
    /// None where it cannot be told.
    pub signed_in: Option<bool>,
    /// The account, partly hidden (ab***@example.com); empty when not known.
    pub account: String,
    /// A folder named by PARAGMA_CLOUDS stands for it.
    pub named: bool,
}

impl State {
    /// Its folder can be used: what is put there goes to the cloud.
    pub fn ready(&self) -> bool {
        self.root.is_some() && self.signed_in != Some(false)
    }

    /// For people: "signed in as ab***@example.com, running".
    pub fn said(&self) -> String {
        if self.named {
            return if self.root.is_some() { "a folder standing for it".into() } else { "not here".into() };
        }
        if !self.installed {
            return "not installed".into();
        }
        if self.signed_in == Some(false) {
            return "installed, no account signed in".into();
        }
        let who = match (self.signed_in, self.account.is_empty()) {
            (Some(true), false) => format!("signed in as {}", self.account),
            (Some(true), true) => "signed in".to_string(),
            _ => "installed".to_string(),
        };
        let runs = if self.running { "running" } else { "not running" };
        format!("{who}, {runs}")
    }

    /// What to do before it can be used; None when it can be.
    pub fn hint(&self) -> Option<String> {
        let n = self.name;
        if self.ready() {
            return None;
        }
        Some(if self.named {
            format!("{n}'s folder named in PARAGMA_CLOUDS is not there")
        } else if !self.installed {
            format!("{n} is not installed on this computer: install it and sign in, or connect a folder")
        } else if self.signed_in == Some(false) {
            format!(
                "{n} is installed, but no account is signed in: what is put in its folder would stay on this computer. Sign in to {n} first"
            )
        } else {
            format!("{n} is not running (its folder is there only while it runs): start {n}")
        })
    }
}

/// The state of every cloud on this computer.
pub fn states() -> Vec<State> {
    if let Some(n) = named() {
        return CLOUDS
            .iter()
            .map(|(id, name)| {
                let root = n.iter().find(|(k, _)| k == id).map(|(_, p)| p.clone()).filter(|p| p.is_dir());
                let here = root.is_some();
                State {
                    id,
                    name,
                    root,
                    installed: here,
                    running: here,
                    signed_in: Some(here),
                    named: true,
                    ..State::default()
                }
            })
            .collect();
    }
    let procs = processes();
    CLOUDS.iter().map(|(id, name)| state_of(id, name, &procs)).collect()
}

/// The state of one cloud.
pub fn state(id: &str) -> State {
    states().into_iter().find(|s| s.id == id).unwrap_or_default()
}

fn state_of(id: &'static str, name: &'static str, procs: &[String]) -> State {
    let root = root(id);
    let mut s = State { id, name, root: root.clone(), ..State::default() };
    let runs = |exe: &str| procs.iter().any(|p| p.eq_ignore_ascii_case(exe));
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from).unwrap_or_default();
    if cfg!(windows) {
        let (local, pf, pf86) = (env("LOCALAPPDATA"), env("ProgramFiles"), env("ProgramFiles(x86)"));
        match id {
            "onedrive" => {
                s.installed = [local.join("Microsoft"), pf.clone(), pf86.clone()].iter().any(|d| {
                    d.join("Microsoft OneDrive").join("OneDrive.exe").is_file()
                        || d.join("OneDrive").join("OneDrive.exe").is_file()
                });
                s.running = runs("OneDrive.exe");
                let accounts = onedrive_accounts();
                s.signed_in = Some(!accounts.is_empty());
                s.account = accounts
                    .iter()
                    .find(|(f, _)| root.as_ref() == Some(f))
                    .or(accounts.first())
                    .map(|(_, e)| mask(e))
                    .unwrap_or_default();
            }
            "gdrive" => {
                s.installed = pf.join("Google").join("Drive File Stream").is_dir();
                s.running = runs("GoogleDriveFS.exe");
                s.signed_in = Some(
                    reg(r"HKCU\Software\Google\DriveFS")
                        .iter()
                        .any(|(_, k, v)| k == "CurrentAccountToken" && !v.is_empty()),
                );
            }
            "dropbox" => {
                s.installed = [local, pf, pf86].iter().any(|d| d.join("Dropbox").is_dir());
                s.running = runs("Dropbox.exe");
                s.signed_in = Some(dropbox().is_some());
            }
            _ => {
                s.running = runs("iCloudDrive.exe") || runs("iCloudServices.exe");
                s.signed_in = Some(root.is_some());
            }
        }
    } else if cfg!(target_os = "macos") {
        let storage: Vec<String> = std::fs::read_dir(home().join("Library").join("CloudStorage"))
            .map(|es| es.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        let signed = |prefix: &str| storage.iter().find(|n| n.starts_with(prefix)).cloned();
        let app = |a: &str| Path::new("/Applications").join(a).is_dir();
        match id {
            "onedrive" => {
                s.installed = app("OneDrive.app");
                s.running = runs("OneDrive");
                s.signed_in = Some(signed("OneDrive").is_some());
            }
            "gdrive" => {
                s.installed = app("Google Drive.app");
                s.running = runs("Google Drive");
                let found = signed("GoogleDrive-");
                s.account = found.as_deref().and_then(|n| n.strip_prefix("GoogleDrive-")).map(mask).unwrap_or_default();
                s.signed_in = Some(found.is_some());
            }
            "dropbox" => {
                s.installed = app("Dropbox.app");
                s.running = runs("Dropbox");
                s.signed_in = Some(dropbox().is_some() || signed("Dropbox").is_some());
            }
            _ => {
                s.installed = true;
                s.running = true;
                s.signed_in = Some(root.is_some());
            }
        }
    } else {
        s.running = runs(id);
    }
    // a folder of it found here tells it is here
    s.installed |= root.is_some();
    s
}

/// The programs running here, by their file's name.
fn processes() -> Vec<String> {
    let out = if cfg!(windows) {
        std::process::Command::new("tasklist").args(["/FO", "CSV", "/NH"]).output()
    } else {
        std::process::Command::new("ps").args(["-A", "-o", "comm="]).output()
    };
    let Ok(out) = out else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if cfg!(windows) {
                l.strip_prefix('"').and_then(|r| r.split('"').next()).map(str::to_string)
            } else {
                l.rsplit('/').next().map(str::to_string)
            }
        })
        .filter(|n| !n.is_empty())
        .collect()
}

/// A Windows registry key and those below it: (key, name, value).
fn reg(key: &str) -> Vec<(String, String, String)> {
    let Ok(out) = std::process::Command::new("reg").args(["query", key, "/s"]).output() else { return Vec::new() };
    let mut at = String::new();
    let mut found = Vec::new();
    for l in String::from_utf8_lossy(&out.stdout).lines() {
        if l.starts_with("HKEY_") {
            at = l.trim().to_string();
        } else if let Some(rest) = l.strip_prefix("    ") {
            let parts: Vec<&str> = rest.split("    ").collect();
            if parts.len() >= 2 {
                found.push((at.clone(), parts[0].to_string(), parts.get(2).copied().unwrap_or("").to_string()));
            }
        }
    }
    found
}

/// OneDrive's accounts signed in here: their folder and e-mail.
fn onedrive_accounts() -> Vec<(PathBuf, String)> {
    let values = reg(r"HKCU\Software\Microsoft\OneDrive\Accounts");
    let mut keys: Vec<&String> = values.iter().map(|(k, _, _)| k).collect();
    keys.dedup();
    keys.into_iter()
        .filter_map(|k| {
            let get = |n: &str| values.iter().find(|(kk, nn, _)| kk == k && nn == n).map(|(_, _, v)| v.clone());
            let folder = PathBuf::from(get("UserFolder")?);
            folder.is_dir().then(|| (folder, get("UserEmail").unwrap_or_default()))
        })
        .collect()
}

/// An account partly hidden: ab***@example.com.
pub fn mask(account: &str) -> String {
    let (user, domain) = account.split_once('@').unwrap_or((account, ""));
    let shown: String = user.chars().take(2).collect();
    if domain.is_empty() { format!("{shown}***") } else { format!("{shown}***@{domain}") }
}

/// Whether a file in a cloud's folder is in the cloud: Windows keeps that
/// for the programs that use its cloud files (OneDrive, Dropbox, iCloud);
/// a file not sent yet is no cloud file at all. None where it cannot be
/// told (another system).
#[cfg(windows)]
pub fn in_cloud(p: &Path) -> Option<bool> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::CloudFilters::{
        CF_PLACEHOLDER_STATE_IN_SYNC, CF_PLACEHOLDER_STATE_INVALID, CF_PLACEHOLDER_STATE_PLACEHOLDER,
        CfGetPlaceholderStateFromFileInfo,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_TAG_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FileAttributeTagInfo, GetFileInformationByHandleEx,
    };
    // Windows shows a program cloud files as plain files unless it asks to
    // see them as they are: asked on this thread while it looks
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlSetThreadPlaceholderCompatibilityMode(mode: i8) -> i8;
    }
    const PHCM_EXPOSE_PLACEHOLDERS: i8 = 2;
    struct Back(i8);
    impl Drop for Back {
        fn drop(&mut self) {
            unsafe { RtlSetThreadPlaceholderCompatibilityMode(self.0) };
        }
    }
    let _back = Back(unsafe { RtlSetThreadPlaceholderCompatibilityMode(PHCM_EXPOSE_PLACEHOLDERS) });
    // its attributes only: nothing brought down, nothing held
    let f = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(p)
        .ok()?;
    let mut info = FILE_ATTRIBUTE_TAG_INFO { FileAttributes: 0, ReparseTag: 0 };
    let ok = unsafe {
        GetFileInformationByHandleEx(
            f.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if ok == 0 {
        return None;
    }
    let state = unsafe {
        CfGetPlaceholderStateFromFileInfo((&info as *const FILE_ATTRIBUTE_TAG_INFO).cast(), FileAttributeTagInfo)
    };
    if state == CF_PLACEHOLDER_STATE_INVALID {
        return None;
    }
    Some(state & CF_PLACEHOLDER_STATE_PLACEHOLDER != 0 && state & CF_PLACEHOLDER_STATE_IN_SYNC != 0)
}

#[cfg(not(windows))]
pub fn in_cloud(_p: &Path) -> Option<bool> {
    None
}

/// Whether the memory's files in a cloud's folder are in the cloud, for
/// people, and whether all is well; what to do first when the cloud cannot
/// take them; None where it cannot be told (another system, Google
/// Drive's own drive).
pub fn upload_line(cloud: &str, path: &str, id: &str) -> Option<(String, bool)> {
    let st = state(cloud);
    if let Some(h) = st.hint() {
        return Some((h, false));
    }
    if st.named || cloud == "gdrive" {
        return None;
    }
    let root = resolve(cloud, path, id)?;
    let mut files = Vec::new();
    super::repo::walk(&root, "", &mut files, &|rel, _| {
        let n = rel.rsplit('/').next().unwrap_or(rel);
        !n.starts_with(".~pg-") && !n.starts_with(".paragma-probe")
    })
    .ok()?;
    let mut waiting = 0;
    for f in &files {
        if !in_cloud(&root.join(f))? {
            waiting += 1;
        }
    }
    let all = crate::output::count(files.len() as i64, "file");
    let name = st.name;
    Some(if waiting == 0 {
        (format!("{name}: all {all} in the cloud"), true)
    } else if st.running {
        (format!("{name}: {waiting} of {all} not in the cloud yet ({name} is sending them)"), true)
    } else {
        (format!("{name}: {waiting} of {all} not in the cloud yet: start {name} to send them"), false)
    })
}

/// The folder a cloud keeps its files in here.
pub fn root(id: &str) -> Option<PathBuf> {
    if let Some(n) = named() {
        return n.into_iter().find(|(k, _)| k == id).map(|(_, p)| p).filter(|p| p.is_dir());
    }
    let h = home();
    let dir = |p: PathBuf| p.is_dir().then_some(p);
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from).and_then(dir);
    // macOS keeps every cloud's folder in ~/Library/CloudStorage/<Name>-<account>
    let storage = |prefix: &str| -> Option<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(h.join("Library").join("CloudStorage"))
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(prefix)))
            .collect();
        found.sort();
        found.into_iter().next()
    };
    match id {
        "onedrive" => env("OneDrive")
            .or_else(|| env("OneDriveConsumer"))
            .or_else(|| env("OneDriveCommercial"))
            .or_else(|| storage("OneDrive"))
            .or_else(|| dir(h.join("OneDrive"))),
        "gdrive" => google_drive().or_else(|| storage("GoogleDrive")).or_else(|| dir(h.join("Google Drive"))),
        "dropbox" => dropbox().or_else(|| storage("Dropbox")).or_else(|| dir(h.join("Dropbox"))),
        "icloud" => dir(h.join("iCloudDrive"))
            .or_else(|| dir(h.join("Library").join("Mobile Documents").join("com~apple~CloudDocs"))),
        _ => None,
    }
}

/// Google Drive for desktop shows the drive as a disk of its own: the one
/// with its shortcuts folder at the top.
fn google_drive() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let drive = (b'D'..=b'Z')
        .map(|l| PathBuf::from(format!("{}:\\", l as char)))
        .find(|r| r.join(".shortcut-targets-by-id").is_dir())?;
    Some(my_drive(&drive).unwrap_or(drive))
}

/// In Google Drive's drive, the folder of the user's own files (My Drive,
/// named in the computer's language; nothing can be put beside it): the
/// only folder there, else the one Google's program gives its first icon
/// (Shared drives has the next one), as each folder's desktop.ini says.
pub fn my_drive(drive: &Path) -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = std::fs::read_dir(drive)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| !e.file_name().to_string_lossy().starts_with(['.', '$']))
        .map(|e| e.path())
        .collect();
    if dirs.len() == 1 {
        return dirs.into_iter().next();
    }
    dirs.into_iter()
        .filter_map(|d| icon(&d.join("desktop.ini")).map(|n| (n, d)))
        .min_by_key(|(n, _)| *n)
        .map(|(_, d)| d)
}

/// The icon number a folder's desktop.ini gives (IconResource=<file>,<n>);
/// the file is UTF-16 or UTF-8.
fn icon(ini: &Path) -> Option<u32> {
    let b = std::fs::read(ini).ok()?;
    // UTF-16 with its mark, or without (Google's: a zero after the first letter)
    let text = if b.starts_with(&[0xFF, 0xFE]) || b.get(1) == Some(&0) {
        let from = if b.starts_with(&[0xFF, 0xFE]) { 2 } else { 0 };
        let units: Vec<u16> = b[from..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(&b).into_owned()
    };
    let line = text.lines().find(|l| l.trim_start().to_ascii_lowercase().starts_with("iconresource="))?;
    line.rsplit(',').next()?.trim().parse().ok()
}

/// Dropbox says where its folder is in its info.json.
fn dropbox() -> Option<PathBuf> {
    let mut places = vec![home().join(".dropbox").join("info.json")];
    for k in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(p) = std::env::var_os(k) {
            places.push(PathBuf::from(p).join("Dropbox").join("info.json"));
        }
    }
    for p in places {
        let Ok(t) = std::fs::read_to_string(&p) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else { continue };
        for kind in ["personal", "business"] {
            if let Some(path) = v[kind]["path"].as_str()
                && Path::new(path).is_dir()
            {
                return Some(PathBuf::from(path));
            }
        }
    }
    None
}

/// The cloud a folder is in, and its path below the cloud's folder with
/// '/'; ("", the whole path) for a folder of no cloud.
pub fn portable(path: &Path) -> (String, String) {
    for c in found() {
        if let Ok(rest) = path.strip_prefix(&c.root) {
            return (c.id.to_string(), rest.to_string_lossy().replace('\\', "/"));
        }
    }
    (String::new(), path.to_string_lossy().into_owned())
}

/// The folder of a memory on this computer: the cloud's folder here and
/// the path below it; when that holds no memory of this id, the memory
/// looked for by its id in that cloud.
pub fn resolve(cloud: &str, rel: &str, id: &str) -> Option<PathBuf> {
    if cloud.is_empty() {
        let p = PathBuf::from(rel);
        return p.is_dir().then_some(p);
    }
    let root = root(cloud)?;
    let p = root.join(rel);
    if marker_id(&p).is_some_and(|m| id.is_empty() || m == id) {
        return Some(p);
    }
    if !id.is_empty()
        && let Some(found) = search(&root, id)
    {
        return Some(found);
    }
    // a new memory there: its folder made on connecting
    (marker_id(&p).is_none()).then_some(p)
}

/// A memory's id, from the marker in a folder.
pub fn marker_id(dir: &Path) -> Option<String> {
    let t = std::fs::read_to_string(dir.join(MARKER)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&t).ok()?;
    Some(v["id"].as_str().unwrap_or("").to_string())
}

/// The folder holding the memory with this id, below `root`: a few
/// levels down, a bounded number of folders looked at.
pub fn search(root: &Path, id: &str) -> Option<PathBuf> {
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0)]);
    let mut looked = 0;
    while let Some((dir, depth)) = queue.pop_front() {
        looked += 1;
        if looked > 3000 {
            return None;
        }
        // no id: any memory
        if marker_id(&dir).is_some_and(|m| id.is_empty() || m == id) {
            return Some(dir);
        }
        if depth >= 3 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name.starts_with('$') {
                continue;
            }
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                queue.push_back((e.path(), depth + 1));
            }
        }
    }
    None
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/sync/cloud.rs"]
mod tests;
