//! What a new version of pg does once, before its first command: brings the
//! instructions `pg setup` wrote into agents' files, the hooks it put into
//! their settings, and the skill copies, up to this version. A stamp in `<home>/version` says which version did
//! it last, so a command costs one small read when nothing changed.

use crate::agents::hooks;
use crate::project::{Home, files};
use crate::skills;
use std::path::PathBuf;

const STAMP: &str = "version";
/// The instruction files `pg setup` wrote to, one per line.
const SETUP_LIST: &str = "setup.txt";
/// The settings files `pg setup` put hooks into: agent, tab, file.
const HOOKS_LIST: &str = "hooks.txt";

/// Runs once per version (and whenever the instructions changed). Never
/// fails a command: what goes wrong is said on stderr.
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
    let _ = skills::write_copies(home);
    for file in set_up_list(home) {
        if skills::installed(&file)
            && let Err(e) = skills::install(&file)
        {
            eprintln!("pg: could not update Paragma's instructions in {}: {e:#}", file.display());
        }
    }
    // the hooks run this pg, wherever this version was put
    if let Ok(exe) = std::env::current_exe() {
        for (agent, file) in hooks_list(home) {
            if hooks::installed(&file)
                && let Err(e) = hooks::install(&agent, &file, &exe)
            {
                eprintln!("pg: could not update Paragma's hooks in {}: {e:#}", file.display());
            }
        }
    }
    let _ = files::write_atomic(&path, format!("{stamp}\n").as_bytes());
}

/// Keeps the file among the ones to update with a new version.
pub fn remember_setup(home: &Home, file: &std::path::Path) {
    let mut all = set_up_list(home);
    if !all.iter().any(|p| p == file) {
        all.push(file.to_path_buf());
        let text: String = all.iter().map(|p| format!("{}\n", p.display())).collect();
        let _ = files::write_atomic(&home.root.join(SETUP_LIST), text.as_bytes());
    }
}

/// Keeps the agent's settings file among the ones whose hooks a new
/// version updates.
pub fn remember_hooks(home: &Home, agent: &str, file: &std::path::Path) {
    let mut all = hooks_list(home);
    if !all.iter().any(|(a, p)| a == agent && p == file) {
        all.push((agent.to_string(), file.to_path_buf()));
        let text: String = all.iter().map(|(a, p)| format!("{a}\t{}\n", p.display())).collect();
        let _ = files::write_atomic(&home.root.join(HOOKS_LIST), text.as_bytes());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_version_updates_the_sections_it_wrote_and_nothing_else() {
        let d = tempfile::tempdir().unwrap();
        let home = Home { root: d.path().join("home") };
        std::fs::create_dir_all(&home.root).unwrap();
        let mine = d.path().join("app").join("AGENTS.md");
        std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
        let old = "# Mine\nKeep this.\n\n<!-- paragma:start -->\nold instructions\n<!-- paragma:end -->\n\nAfter.\n";
        std::fs::write(&mine, old).unwrap();
        remember_setup(&home, &mine);
        remember_setup(&home, &mine);
        assert_eq!(set_up_list(&home).len(), 1);
        let theirs = d.path().join("other").join("AGENTS.md");
        std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
        std::fs::write(&theirs, "no section here\n").unwrap();
        remember_setup(&home, &theirs);

        run(&home);
        let text = std::fs::read_to_string(&mine).unwrap();
        assert!(text.starts_with("# Mine\nKeep this.\n\n<!-- paragma:start -->\n## Memory (Paragma)"));
        assert!(text.ends_with("<!-- paragma:end -->\n\nAfter.\n"));
        assert!(!text.contains("old instructions"));
        assert_eq!(std::fs::read_to_string(&theirs).unwrap(), "no section here\n");
        assert!(home.root.join("skills").join("paragma-coding").join("SKILL.md").is_file());

        // stamped: the next command leaves the files alone
        std::fs::write(&mine, old).unwrap();
        run(&home);
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), old);
    }

    #[test]
    fn an_older_version_leaves_a_newer_ones_instructions_alone() {
        let d = tempfile::tempdir().unwrap();
        let home = Home { root: d.path().join("home") };
        std::fs::create_dir_all(&home.root).unwrap();
        let mine = d.path().join("AGENTS.md");
        let newer = "<!-- paragma:start -->\nnewer instructions\n<!-- paragma:end -->\n";
        std::fs::write(&mine, newer).unwrap();
        remember_setup(&home, &mine);
        std::fs::write(home.root.join(STAMP), "99.0.0 0000000000000000\n").unwrap();
        run(&home);
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), newer);
        assert!(numbers("0.2.10") > numbers("0.2.9"));
        assert!(numbers("0.3.0-rc.1") > numbers("0.2.9"));
        assert_eq!(numbers(""), [0]);
    }
}
