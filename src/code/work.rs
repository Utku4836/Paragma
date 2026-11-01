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

/// The package manager of a Node project, from its lock file.
fn node_manager(dir: &Path) -> &'static str {
    let has = |f: &str| dir.join(f).exists();
    if has("pnpm-lock.yaml") {
        "pnpm"
    } else if has("yarn.lock") {
        "yarn"
    } else if has("bun.lockb") || has("bun.lock") {
        "bun"
    } else {
        "npm"
    }
}

/// What a project is built with, from the files at its root: "go 1.22",
/// "node (pnpm; scripts: dev, build, test)", "make: build, test", ...
pub fn stack(dir: &Path) -> Vec<String> {
    let has = |f: &str| dir.join(f).exists();
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap_or_default();
    let mut out = Vec::new();
    if has("go.mod") {
        let version = read("go.mod").lines().find_map(|l| l.strip_prefix("go ").map(|v| v.trim().to_string()));
        out.push(format!("go{}", version.map(|v| format!(" {v}")).unwrap_or_default()));
    }
    if has("Cargo.toml") {
        out.push("rust (cargo)".to_string());
    }
    if has("package.json") {
        let v: serde_json::Value = serde_json::from_str(&read("package.json")).unwrap_or_default();
        let scripts: Vec<String> =
            v["scripts"].as_object().map(|m| m.keys().take(6).cloned().collect()).unwrap_or_default();
        let scripts = if scripts.is_empty() { String::new() } else { format!("; scripts: {}", scripts.join(", ")) };
        out.push(format!("node ({}{scripts})", node_manager(dir)));
    }
    let python = if has("uv.lock") {
        Some("uv")
    } else if has("poetry.lock") {
        Some("poetry")
    } else if has("requirements.txt") {
        Some("pip")
    } else if has("pyproject.toml") || has("setup.py") {
        Some("pyproject")
    } else {
        None
    };
    if let Some(p) = python {
        out.push(format!("python ({p})"));
    }
    if has("Makefile") {
        let targets: Vec<String> = read("Makefile")
            .lines()
            .filter_map(|l| l.split_once(':').map(|(t, _)| t))
            .filter(|t| {
                !t.is_empty()
                    && !t.starts_with(['.', '\t', ' ', '#'])
                    && t.chars().all(|c| c.is_alphanumeric() || "-_/".contains(c))
            })
            .take(8)
            .map(str::to_string)
            .collect();
        out.push(if targets.is_empty() { "make".to_string() } else { format!("make: {}", targets.join(", ")) });
    }
    if has("docker-compose.yml") || has("docker-compose.yaml") || has("compose.yaml") || has("compose.yml") {
        out.push("docker compose".to_string());
    } else if has("Dockerfile") {
        out.push("docker".to_string());
    }
    out
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
        let test = match node_manager(dir) {
            "pnpm" => "pnpm test",
            "yarn" => "yarn test",
            "bun" => "bun test",
            _ => "npm test",
        };
        return v["scripts"]["test"].is_string().then_some(test);
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
