//! The coding agents pg can set up, and where each reads its instructions:
//! a global file read in every conversation, and a file it reads from a
//! conversation's folder.

mod conversations;

pub use conversations::Conversation;

use std::path::{Path, PathBuf};

pub struct Agent {
    pub id: &'static str,
    pub name: &'static str,
    /// Read in every conversation.
    pub global: PathBuf,
    /// Read from the folder a conversation works in.
    pub folder_file: &'static str,
    /// The apps of it found on this computer: "CLI", "desktop", "IDE".
    pub apps: Vec<&'static str>,
    /// Found on this computer.
    pub installed: bool,
}

impl Agent {
    /// Its conversations whose folders still exist, newest first.
    pub fn conversations(&self) -> Vec<Conversation> {
        conversations::of(self.id)
    }
}

fn on_path(names: &[&str]) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    let exts: Vec<String> = if cfg!(windows) {
        vec![".exe".into(), ".cmd".into(), ".bat".into(), String::new()]
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&path)
        .any(|dir| names.iter().any(|n| exts.iter().any(|e| dir.join(format!("{n}{e}")).is_file())))
}

fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_default()
}

/// %LocalAppData% and %AppData% on Windows (empty elsewhere).
fn app_data() -> (PathBuf, PathBuf) {
    let get = |k| std::env::var_os(k).map(PathBuf::from).unwrap_or_default();
    (get("LOCALAPPDATA"), get("APPDATA"))
}

/// Every agent pg can set up, installed or not.
pub fn known() -> Vec<Agent> {
    let home = home();
    let (local, roaming) = app_data();
    let exists = |p: PathBuf| !p.as_os_str().is_empty() && p.exists();
    let apps = |found: &[(bool, &'static str)]| -> Vec<&'static str> {
        found.iter().filter(|(yes, _)| *yes).map(|(_, app)| *app).collect()
    };
    let mac = |app: &str| exists(PathBuf::from("/Applications").join(app));
    let local_set = !local.as_os_str().is_empty();
    let agent = |id, name, global: PathBuf, folder_file, apps: Vec<&'static str>, data: PathBuf| Agent {
        id,
        name,
        global,
        folder_file,
        installed: !apps.is_empty() || data.exists(),
        apps,
    };
    vec![
        agent(
            "claude",
            "Claude Code",
            home.join(".claude").join("CLAUDE.md"),
            "CLAUDE.md",
            apps(&[
                (on_path(&["claude"]), "CLI"),
                (
                    (local_set && (exists(local.join("AnthropicClaude")) || exists(local.join("Claude"))))
                        || exists(roaming.join("Claude"))
                        || mac("Claude.app"),
                    "desktop",
                ),
            ]),
            home.join(".claude"),
        ),
        agent(
            "codex",
            "Codex",
            home.join(".codex").join("AGENTS.md"),
            "AGENTS.md",
            apps(&[
                (on_path(&["codex"]), "CLI"),
                (exists(home.join(".codex").join(".desktop-created")) || mac("Codex.app"), "desktop"),
            ]),
            home.join(".codex"),
        ),
        agent(
            "antigravity",
            "Antigravity",
            home.join(".gemini").join("AGENTS.md"),
            "AGENTS.md",
            apps(&[
                (on_path(&["agy"]) || exists(home.join(".gemini").join("antigravity-cli")), "CLI"),
                (
                    exists(home.join(".gemini").join("antigravity"))
                        || (local_set && exists(local.join("Programs").join("Antigravity")))
                        || mac("Antigravity.app"),
                    "IDE",
                ),
            ]),
            home.join(".gemini").join("antigravity"),
        ),
        agent(
            "opencode",
            "OpenCode",
            home.join(".config").join("opencode").join("AGENTS.md"),
            "AGENTS.md",
            apps(&[(on_path(&["opencode"]) || exists(home.join(".config").join("opencode")), "CLI")]),
            home.join(".local").join("share").join("opencode"),
        ),
    ]
}

pub fn find(id: &str) -> Option<Agent> {
    known().into_iter().find(|a| a.id.eq_ignore_ascii_case(id) || a.name.eq_ignore_ascii_case(id))
}

/// A path with the home folder as ~.
pub fn short(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    match std::env::home_dir() {
        Some(h) => {
            let h = h.to_string_lossy().replace('\\', "/");
            match s.get(..h.len()) {
                Some(head) if !h.is_empty() && head.eq_ignore_ascii_case(&h) => format!("~{}", &s[h.len()..]),
                _ => s,
            }
        }
        None => s,
    }
}
