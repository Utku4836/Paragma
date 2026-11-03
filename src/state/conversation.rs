//! Which conversation is calling.
//!
//! An agent runs every tool call in a fresh shell, so pg's parent changes
//! from call to call, but the first ancestor that is not a shell is the
//! agent itself and stays the same for the whole conversation. Its name and
//! pid are the conversation's key. Agents that name the conversation in the
//! environment of the commands they run (Claude Code, Codex) are known by
//! that name instead: the same one their hooks are given, and it lasts when
//! a conversation is resumed. $PARAGMA_SESSION overrides both.

use std::sync::OnceLock;

/// Shells and launchers that sit between an agent and pg.
const SHELLS: &[&str] = &[
    "pwsh",
    "powershell",
    "cmd",
    "bash",
    "sh",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "tcsh",
    "csh",
    "env",
    "timeout",
    "conhost",
    "winpty",
    "mintty",
    "git-bash",
    "nu",
    "xonsh",
    "sudo",
    "nice",
    "time",
];

/// The variables agents name their conversation in: (agent, variable).
const AGENT_SESSIONS: &[(&str, &str)] = &[("claude", "CLAUDE_CODE_SESSION_ID"), ("codex", "CODEX_THREAD_ID")];

/// The key of an agent's conversation known by its id (as hooks know it).
pub fn of_agent(agent: &str, id: &str) -> String {
    format!("{agent}:{id}")
}

/// The calling conversation's key, "" when it cannot be told.
pub fn key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| {
        if let Ok(k) = std::env::var("PARAGMA_SESSION")
            && !k.is_empty()
        {
            return k;
        }
        for (agent, var) in AGENT_SESSIONS {
            if let Ok(id) = std::env::var(var)
                && !id.is_empty()
            {
                return of_agent(agent, &id);
            }
        }
        let table = process::Table::read();
        let mut pid = table.parent(std::process::id());
        for _ in 0..12 {
            let Some(p) = pid else { break };
            let Some((name, parent)) = table.info(p) else {
                break;
            };
            let base = name.rsplit(['/', '\\']).next().unwrap_or(&name).to_lowercase();
            let base = base.strip_suffix(".exe").unwrap_or(&base).to_string();
            if !SHELLS.contains(&base.as_str()) {
                return format!("{base}:{p}");
            }
            pid = Some(parent);
        }
        String::new()
    })
}

#[cfg(windows)]
mod process {
    use std::collections::HashMap;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };

    /// Every process: pid -> (executable name, parent pid).
    pub struct Table(HashMap<u32, (String, u32)>);

    impl Table {
        pub fn read() -> Table {
            let mut map = HashMap::new();
            // SAFETY: plain Win32 calls on a snapshot handle we own and close
            unsafe {
                let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
                if snap == INVALID_HANDLE_VALUE {
                    return Table(map);
                }
                let mut e: PROCESSENTRY32W = std::mem::zeroed();
                e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
                let mut ok = Process32FirstW(snap, &mut e) != 0;
                while ok {
                    let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                    let name = String::from_utf16_lossy(&e.szExeFile[..len]);
                    map.insert(e.th32ProcessID, (name, e.th32ParentProcessID));
                    ok = Process32NextW(snap, &mut e) != 0;
                }
                CloseHandle(snap);
            }
            Table(map)
        }

        pub fn parent(&self, pid: u32) -> Option<u32> {
            self.0.get(&pid).map(|e| e.1)
        }

        pub fn info(&self, pid: u32) -> Option<(String, u32)> {
            self.0.get(&pid).cloned()
        }
    }
}

#[cfg(not(windows))]
mod process {
    /// Processes looked up one at a time: /proc on Linux, ps elsewhere.
    pub struct Table;

    impl Table {
        pub fn read() -> Table {
            Table
        }

        pub fn parent(&self, pid: u32) -> Option<u32> {
            if pid == std::process::id() {
                return Some(std::os::unix::process::parent_id());
            }
            self.info(pid).map(|i| i.1)
        }

        pub fn info(&self, pid: u32) -> Option<(String, u32)> {
            if let Ok(s) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                let (open, close) = (s.find('(')?, s.rfind(')')?);
                let ppid = s[close + 1..].split_whitespace().nth(1)?.parse().ok()?;
                return Some((s[open + 1..close].to_string(), ppid));
            }
            let out =
                std::process::Command::new("ps").args(["-o", "ppid=,comm=", "-p", &pid.to_string()]).output().ok()?;
            let text = String::from_utf8_lossy(&out.stdout);
            let mut f = text.split_whitespace();
            let ppid = f.next()?.parse().ok()?;
            Some((f.collect::<Vec<_>>().join(" "), ppid))
        }
    }
}
