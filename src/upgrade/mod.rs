//! What a new version of pg does once, before its first command: brings what
//! `pg setup` wrote up to this version: Paragma's section in agents'
//! instruction files (only pg's own lines; the user's stay), its hooks in
//! their settings, its MCP server in the apps (the pg they start, which an
//! update may have moved), and the skill copies. A stamp in `<home>/version`
//! says which version did it last, so a command costs one small read when
//! nothing changed. `pg setup --update` does the same at any time and says
//! what it did.
//!
//! What to update: the files `pg setup` noted when it wrote them, and, for
//! the memory in its usual place (no PARAGMA_HOME), also each agent's and
//! app's global file that holds Paragma's part, noted or not (written by an
//! older pg that kept no list, or by hand).

use crate::agents::{self, hooks, mcp};
use crate::project::{Home, files};
use crate::skills;
use std::path::{Path, PathBuf};

const STAMP: &str = "version";
/// The instruction files `pg setup` wrote to, one per line.
const SETUP_LIST: &str = "setup.txt";
/// The settings files `pg setup` put hooks into: agent, tab, file.
const HOOKS_LIST: &str = "hooks.txt";
/// The apps `pg setup` put the MCP server into, one id per line.
const MCP_LIST: &str = "mcp.txt";

/// Runs once per version (and whenever the instructions changed). Never
/// fails a command: what goes wrong, or keeps the user's lines aside, is
/// said on stderr.
pub fn run(home: &Home) {
    let stamp = format!("{} {:016x}", env!("CARGO_PKG_VERSION"), instructions_hash());
    let path = home.root.join(STAMP);
    let found = std::fs::read_to_string(&path).unwrap_or_default();
    if found.trim() == stamp {
        return;
    }
    // a newer pg wrote its instructions: an older one (a version taken back)
    // leaves them as they are
    let found_version = found.split_whitespace().next().unwrap_or("");
    if numbers(found_version) > numbers(env!("CARGO_PKG_VERSION")) {
        return;
    }
    if !home.root.is_dir() {
        return; // nothing set up yet: the first command that needs a home makes it
    }
    for done in update(home) {
        match &done.outcome {
            Outcome::Failed(e) => eprintln!("pg: could not update Paragma's {} in {}: {e}", done.what, done.place),
            Outcome::Written { kept } if *kept > 0 => eprintln!(
                "pg: updated Paragma's section in {}; {} you wrote inside it now below it",
                done.place,
                crate::output::count(*kept as i64, "line")
            ),
            _ => {}
        }
    }
    let _ = files::write_atomic(&path, format!("{stamp}\n").as_bytes());
}

/// What one update did.
pub struct Done {
    /// "instructions", "hooks", "MCP server"
    pub what: &'static str,
    /// The file, or the app.
    pub place: String,
    pub outcome: Outcome,
}

pub enum Outcome {
    Unchanged,
    /// `kept`: the user's own lines found inside Paragma's section, kept
    /// below it.
    Written {
        kept: usize,
    },
    Failed(String),
}

/// Brings everything pg set up to this version; what it did, one by one.
pub fn update(home: &Home) -> Vec<Done> {
    let _ = skills::write_copies(home);
    let everywhere = usual_place(home);
    let mut out = Vec::new();

    let mut sections = set_up_list(home);
    if everywhere {
        add_new(&mut sections, agents::known().into_iter().map(|a| a.global));
    }
    for file in sections.into_iter().filter(|f| skills::installed(f)) {
        let outcome = match skills::install(&file) {
            Ok(skills::Written::Unchanged) => Outcome::Unchanged,
            Ok(skills::Written::Written { kept }) => Outcome::Written { kept },
            Err(e) => Outcome::Failed(format!("{e:#}")),
        };
        out.push(Done { what: "instructions", place: agents::short(&file), outcome });
    }

    // the hooks and the servers run this pg, wherever this version was put
    let Ok(exe) = std::env::current_exe() else { return out };
    let mut with_hooks = hooks_list(home);
    if everywhere {
        for agent in hooks::AGENTS {
            let file = hooks::file(agent, None);
            if !with_hooks.iter().any(|(a, p)| a == agent && *p == file) {
                with_hooks.push((agent.to_string(), file));
            }
        }
    }
    for (agent, file) in with_hooks.into_iter().filter(|(_, f)| hooks::installed(f)) {
        let outcome = match hooks::install(&agent, &file, &exe) {
            Ok(hooks::Change::Unchanged) => Outcome::Unchanged,
            Ok(hooks::Change::Written) => Outcome::Written { kept: 0 },
            Err(e) => Outcome::Failed(format!("{e:#}")),
        };
        out.push(Done { what: "hooks", place: agents::short(&file), outcome });
    }

    let noted = mcp_list(home);
    for c in mcp::clients().into_iter().filter(|c| everywhere || noted.iter().any(|n| n == c.id)) {
        if !mcp::installed(&c) {
            continue;
        }
        let outcome = if mcp::runs(&c, &exe) {
            Outcome::Unchanged
        } else {
            match mcp::install(&c, &exe) {
                Ok(_) => Outcome::Written { kept: 0 },
                Err(e) => Outcome::Failed(format!("{e:#}")),
            }
        };
        out.push(Done { what: "MCP server", place: c.name.to_string(), outcome });
    }
    out
}

/// The memory in its usual place, ~/.paragma: the agents' global files are
/// its own. A memory elsewhere (PARAGMA_HOME: a test, a second memory)
/// updates only what was set up for it.
fn usual_place(home: &Home) -> bool {
    std::env::var_os("PARAGMA_HOME").is_none_or(|p| p.is_empty())
        && std::env::home_dir().is_some_and(|h| h.join(".paragma") == home.root)
}

fn add_new(list: &mut Vec<PathBuf>, more: impl Iterator<Item = PathBuf>) {
    for p in more {
        if !p.as_os_str().is_empty() && !list.contains(&p) {
            list.push(p);
        }
    }
}

/// Keeps the file among the ones to update with a new version.
pub fn remember_setup(home: &Home, file: &Path) {
    let mut all = set_up_list(home);
    if !all.iter().any(|p| p == file) {
        all.push(file.to_path_buf());
        let text: String = all.iter().map(|p| format!("{}\n", p.display())).collect();
        let _ = files::write_atomic(&home.root.join(SETUP_LIST), text.as_bytes());
    }
}

/// Keeps the agent's settings file among the ones whose hooks a new
/// version updates.
pub fn remember_hooks(home: &Home, agent: &str, file: &Path) {
    let mut all = hooks_list(home);
    if !all.iter().any(|(a, p)| a == agent && p == file) {
        all.push((agent.to_string(), file.to_path_buf()));
        let text: String = all.iter().map(|(a, p)| format!("{a}\t{}\n", p.display())).collect();
        let _ = files::write_atomic(&home.root.join(HOOKS_LIST), text.as_bytes());
    }
}

/// Keeps the app among the ones whose MCP server a new version updates.
pub fn remember_mcp(home: &Home, client: &str) {
    let mut all = mcp_list(home);
    if !all.iter().any(|c| c == client) {
        all.push(client.to_string());
        let text: String = all.iter().map(|c| format!("{c}\n")).collect();
        let _ = files::write_atomic(&home.root.join(MCP_LIST), text.as_bytes());
    }
}

fn hooks_list(home: &Home) -> Vec<(String, PathBuf)> {
    std::fs::read_to_string(home.root.join(HOOKS_LIST))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(a, p)| (a.to_string(), PathBuf::from(p)))
        .collect()
}

fn set_up_list(home: &Home) -> Vec<PathBuf> {
    std::fs::read_to_string(home.root.join(SETUP_LIST))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

fn mcp_list(home: &Home) -> Vec<String> {
    std::fs::read_to_string(home.root.join(MCP_LIST))
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// "0.2.10" -> [0, 2, 10], to compare versions; a suffix such as "-rc.1" is
/// left out.
fn numbers(version: &str) -> Vec<u64> {
    version.split('-').next().unwrap_or("").split('.').map(|n| n.parse().unwrap_or(0)).collect()
}

/// Changes when the section or a skill changes, also within one version.
fn instructions_hash() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut add = |s: &str| {
        for b in s.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
    };
    add(&skills::section());
    for (name, text) in skills::SKILLS {
        add(name);
        add(text);
    }
    h
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/upgrade/mod.rs"]
mod tests;
