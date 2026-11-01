//! What agents are told. The core goes into each agent's instructions file
//! through `pg setup` and is read in every conversation: it is short and
//! says when to use pg. The skills are read when needed with `pg skill
//! <name>`; a copy of each is kept in `<home>/skills/paragma-<name>/SKILL.md`
//! so it can be linked into an agent's own skills folder.

use crate::project::{Home, files};
use anyhow::Result;

pub const CORE: &str = include_str!("core.md");

/// (name, text)
pub const SKILLS: &[(&str, &str)] = &[
    ("coding", include_str!("coding.md")),
    ("projects", include_str!("projects.md")),
    ("knowledge", include_str!("knowledge.md")),
];

const START: &str = "<!-- paragma:start -->";
const END: &str = "<!-- paragma:end -->";

/// The section written into an agent's instructions file.
pub fn section() -> String {
    format!("{START}\n## Memory (Paragma)\n\n{CORE}{END}\n")
}

/// Writes (or replaces) Paragma's section in an instructions file, keeping
/// the rest of it. Returns whether the file changed.
pub fn install(path: &std::path::Path) -> Result<bool> {
    let old = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let section = section();
    let text = match (old.find(START), old.find(END)) {
        (Some(i), Some(j)) if j > i => {
            let mut end = j + END.len();
            if old[end..].starts_with('\n') {
                end += 1;
            }
            if old[i..end] == section {
                return Ok(false);
            }
            format!("{}{section}{}", &old[..i], &old[end..])
        }
        _ if old.trim().is_empty() => section,
        _ => format!("{}\n\n{section}", old.trim_end()),
    };
    files::write_atomic(path, text.as_bytes())?;
    Ok(true)
}

/// Whether an instructions file has Paragma's section (any version).
pub fn installed(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|t| t.contains(START))
}

/// Keeps `<home>/skills/paragma-<name>/SKILL.md` equal to this version.
pub fn write_copies(home: &Home) -> Result<()> {
    for (name, text) in SKILLS {
        let path = home.root.join("skills").join(format!("paragma-{name}")).join("SKILL.md");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(*text) {
            files::write_atomic(&path, text.as_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_replaces_itself_and_keeps_the_rest() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("AGENTS.md");
        std::fs::write(&p, "# My rules\nBe brief.\n").unwrap();
        assert!(install(&p).unwrap());
        assert!(!install(&p).unwrap());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("# My rules\nBe brief.\n\n<!-- paragma:start -->"));
        std::fs::write(&p, text.replace("Memory lives in", "OLD")).unwrap();
        assert!(install(&p).unwrap());
        let again = std::fs::read_to_string(&p).unwrap();
        assert_eq!(again.matches(START).count(), 1);
        assert!(again.contains("Memory lives in") && !again.contains("OLD"));
    }

    #[test]
    fn the_core_stays_short() {
        assert!(crate::output::tokens(CORE) < 450, "{} tokens", crate::output::tokens(CORE));
    }
}
