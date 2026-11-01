//! Projects on disk: each one is a folder of notes with its own `.pg/`
//! folder for the index and the state.
//!
//! ```text
//! ~/.paragma/projects/
//!   game-engine/
//!     project.md  rules.md  physics/collisions.md ...
//!     .pg/index.db  .pg/state.db  .pg/lock
//! ```

pub mod env;
pub mod files;
pub mod names;
pub mod plan;
pub mod scan;

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

/// Where Paragma keeps everything: $PARAGMA_HOME, else ~/.paragma.
pub struct Home {
    pub root: PathBuf,
}

impl Home {
    pub fn locate() -> Home {
        let root = match std::env::var_os("PARAGMA_HOME") {
            Some(p) if !p.is_empty() => PathBuf::from(p),
            _ => std::env::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".paragma"),
        };
        Home { root }
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }

    /// Every project, by name.
    pub fn projects(&self) -> Vec<Project> {
        let mut out: Vec<Project> = std::fs::read_dir(self.projects_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|n| !n.starts_with('.'))
            .map(|name| self.project_at(&name))
            .collect();
        out.sort_by_key(|p| p.name.to_lowercase());
        out
    }

    pub fn project(&self, name: &str) -> Option<Project> {
        let p = self.project_at(name);
        p.dir.is_dir().then_some(p)
    }

    pub fn create_project(&self, name: &str) -> Result<Project> {
        check_name(name)?;
        let p = self.project_at(name);
        std::fs::create_dir_all(p.meta_dir())?;
        env::create(&p)?;
        Ok(p)
    }

    fn project_at(&self, name: &str) -> Project {
        Project { name: name.to_string(), dir: self.projects_dir().join(name) }
    }
}

#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    pub dir: PathBuf,
}

impl Project {
    /// The `.pg/` folder: index, state and the write lock.
    pub fn meta_dir(&self) -> PathBuf {
        self.dir.join(".pg")
    }

    pub fn index_path(&self) -> PathBuf {
        self.meta_dir().join("index.db")
    }

    pub fn state_path(&self) -> PathBuf {
        self.meta_dir().join("state.db")
    }

    /// A note's file from its path in the project ("physics/collisions"
    /// or "physics/collisions.md").
    pub fn note_file(&self, rel: &str) -> PathBuf {
        let rel = rel.trim_matches('/');
        let rel = if rel.ends_with(".md") { rel.to_string() } else { format!("{rel}.md") };
        self.dir.join(rel)
    }

    /// The path in the project of a file below its folder, with '/'.
    pub fn rel(&self, file: &Path) -> String {
        file.strip_prefix(&self.dir).unwrap_or(file).to_string_lossy().replace('\\', "/")
    }
}

/// Names of projects, folders and notes: anything a file system on every
/// platform accepts, without path separators.
pub fn check_name(name: &str) -> Result<()> {
    let bad = |c: char| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control();
    if name.trim().is_empty() || name != name.trim() || name.starts_with('.') || name.chars().any(bad) {
        bail!("{name:?} cannot be a name: use letters, digits, spaces, '-' or '_'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(check_name("game-engine").is_ok());
        assert!(check_name("Oyun Motoru").is_ok());
        assert!(check_name("a/b").is_err());
        assert!(check_name(".pg").is_err());
        assert!(check_name(" x").is_err());
    }

    #[test]
    fn note_files() {
        let p = Project { name: "g".into(), dir: PathBuf::from("/h/g") };
        assert_eq!(p.note_file("physics/collisions"), PathBuf::from("/h/g/physics/collisions.md"));
        assert_eq!(p.note_file("/rules.md"), PathBuf::from("/h/g/rules.md"));
        assert_eq!(p.rel(&PathBuf::from("/h/g/physics/collisions.md")), "physics/collisions.md");
    }
}
