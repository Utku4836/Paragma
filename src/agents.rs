//! The coding agents pg can set up, and where each reads its instructions:
//! a global file read in every conversation, and a file it reads from a
//! conversation's folder.

use std::path::{Path, PathBuf};

pub struct Agent {
    pub id: &'static str,
    pub name: &'static str,
    /// Read in every conversation.
    pub global: PathBuf,
    /// Read from the folder a conversation works in.
    pub folder_file: &'static str,
    /// Found on this computer.
    pub installed: bool,
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

/// Every agent pg can set up, installed or not.
pub fn known() -> Vec<Agent> {
    let home = std::env::home_dir().unwrap_or_default();
    let exists = |p: &Path| p.exists();
    vec![
        Agent {
            id: "claude",
            name: "Claude Code",
            global: home.join(".claude").join("CLAUDE.md"),
            folder_file: "CLAUDE.md",
            installed: on_path(&["claude"]) || exists(&home.join(".claude")),
        },
        Agent {
            id: "codex",
            name: "Codex",
            global: home.join(".codex").join("AGENTS.md"),
            folder_file: "AGENTS.md",
            installed: on_path(&["codex"]) || exists(&home.join(".codex")),
        },
        Agent {
            id: "antigravity",
            name: "Antigravity",
            global: home.join(".gemini").join("AGENTS.md"),
            folder_file: "AGENTS.md",
            installed: on_path(&["agy", "antigravity"]) || exists(&home.join(".gemini").join("antigravity")),
        },
        Agent {
            id: "opencode",
            name: "OpenCode",
            global: home.join(".config").join("opencode").join("AGENTS.md"),
            folder_file: "AGENTS.md",
            installed: on_path(&["opencode"]) || exists(&home.join(".config").join("opencode")),
        },
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
            match s.strip_prefix(&h) {
                Some(rest) => format!("~{rest}"),
                None => s,
            }
        }
        None => s,
    }
}
