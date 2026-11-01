//! The code around the work at hand: what changed since the last commit
//! (down to the function), how the project runs its tests, and the code a
//! task is likely to touch.

use super::{CodeIndex, Sym, git};
use anyhow::Result;
use std::path::Path;

/// A change since the last commit: a symbol whose lines changed, or a new
/// file (no symbol).
#[derive(Clone, Debug)]
pub struct Change {
    pub folder: String,
    pub path: String,
    pub symbol: Option<Sym>,
}

impl Change {
    pub fn show(&self, code: &CodeIndex) -> String {
        let file = code.display(&self.folder, &self.path);
        match &self.symbol {
            Some(s) => format!("{file}#{}", s.qualified),
            None => format!("{file} (new)"),
        }
    }
}

/// The functions, methods and types changed since the last commit, in every
/// code folder.
pub fn changed(code: &CodeIndex) -> Result<Vec<Change>> {
    let mut out = Vec::new();
    for f in &code.folders {
        for (path, ranges) in git::changed(&f.dir) {
            let Some(ranges) = ranges else {
                out.push(Change { folder: f.key.clone(), path, symbol: None });
                continue;
            };
            let symbols = code.symbols_of(&f.key, &path)?;
            // the innermost symbols the changed lines fall in
            let mut hit: Vec<Sym> = Vec::new();
            for (a, b) in ranges {
                let inner = symbols.iter().filter(|s| s.start <= b && s.end >= a).filter(|s| {
                    !symbols
                        .iter()
                        .any(|t| t.qualified.starts_with(&format!("{}.", s.qualified)) && t.start <= b && t.end >= a)
                });
                for s in inner {
                    if !hit.iter().any(|h| h.qualified == s.qualified) {
                        hit.push(s.clone());
                    }
                }
            }
            if hit.is_empty() && !symbols.is_empty() {
                continue; // only blank lines, comments or imports changed
            }
            if hit.is_empty() {
                out.push(Change { folder: f.key.clone(), path: path.clone(), symbol: None });
            }
            out.extend(hit.into_iter().map(|s| Change { folder: f.key.clone(), path: path.clone(), symbol: Some(s) }));
        }
    }
    Ok(out)
}

/// The test command for one file's tests only, where the tool allows it:
/// go test ./payments/, cargo test refund, pytest tests/test_refund.py.
pub fn narrow_test(cmd: &str, dir: &Path, file: &str) -> Option<String> {
    let (folder, name) = file.rsplit_once('/').unwrap_or(("", file));
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches("_test");
    match cmd {
        "go test ./..." => {
            Some(if folder.is_empty() { "go test .".to_string() } else { format!("go test ./{folder}/") })
        }
        "cargo test" => Some(format!("cargo test {stem}")),
        "pytest" => {
            let stem = stem.trim_start_matches("test_");
            [format!("tests/test_{stem}.py"), format!("test_{stem}.py"), format!("{folder}/test_{stem}.py")]
                .into_iter()
                .find(|t| dir.join(t).is_file())
                .map(|t| format!("pytest {t}"))
        }
        _ => None,
    }
}

/// How a project runs its tests, from the files at its root.
pub fn test_command(dir: &Path) -> Option<&'static str> {
    let has = |f: &str| dir.join(f).exists();
    if has("go.mod") {
        return Some("go test ./...");
    }
    if has("Cargo.toml") {
        return Some("cargo test");
    }
    if has("package.json") {
        let pkg = std::fs::read_to_string(dir.join("package.json")).unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&pkg).unwrap_or_default();
        return v["scripts"]["test"].is_string().then_some("npm test");
    }
    if has("pytest.ini")
        || has("pyproject.toml")
        || has("setup.cfg")
        || has("tox.ini")
        || has("tests") && has("setup.py")
    {
        return Some("pytest");
    }
    if has("pom.xml") {
        return Some("mvn test");
    }
    if has("build.gradle") || has("build.gradle.kts") {
        return Some(if has("gradlew") { "./gradlew test" } else { "gradle test" });
    }
    if has("Gemfile") && has("spec") {
        return Some("bundle exec rspec");
    }
    if has("pubspec.yaml") {
        return Some("flutter test");
    }
    if has("build.zig") {
        return Some("zig build test");
    }
    if has("Package.swift") {
        return Some("swift test");
    }
    let dotnet = std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| {
        e.file_name().to_string_lossy().ends_with(".sln") || e.file_name().to_string_lossy().ends_with(".csproj")
    });
    dotnet.then_some("dotnet test")
}
