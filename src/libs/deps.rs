//! The libraries a code folder uses, at the versions it uses, and where
//! their source already is on this computer: Rust (Cargo.lock and cargo's
//! registry), JavaScript and TypeScript (node_modules), Go (go.mod and the
//! module cache), Python (the virtual environment's site-packages).
//! Nothing is downloaded.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// One library a folder uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Dep {
    /// rust, npm, go, python
    pub eco: &'static str,
    /// As the project names it: tree-sitter, @types/node, github.com/spf13/cobra, requests.
    pub name: String,
    pub version: String,
    /// The folder of its source; None when it is not on this computer.
    pub dir: Option<PathBuf>,
    /// Named by the project itself (not only by another library).
    pub direct: bool,
    /// Rust: the features the project asks for, and whether the default
    /// ones come too; None when they cannot be told.
    pub features: Option<(Vec<String>, bool)>,
}

impl Dep {
    /// Rust: the features turned on (asked for, and the crate's defaults
    /// unless turned off); None when they cannot be told. Reads the
    /// crate's Cargo.toml, so only when a feature is to be shown.
    pub fn features_on(&self) -> Option<HashSet<String>> {
        let (asked, defaults) = self.features.as_ref()?;
        let mut on: HashSet<String> = asked.iter().cloned().collect();
        if *defaults && let Some(d) = &self.dir {
            on.extend(crate_defaults(d));
        }
        Some(on)
    }

    /// The name as code writes it: tree_sitter, cobra, requests.
    pub fn code_name(&self) -> String {
        match self.eco {
            "rust" => self.name.replace('-', "_"),
            "go" => self.name.rsplit('/').next().unwrap_or(&self.name).to_string(),
            "python" => self.name.to_lowercase().replace('-', "_"),
            _ => self.name.clone(),
        }
    }

    /// Whether a word of a query or an error names this library.
    pub fn named_by(&self, word: &str) -> bool {
        let w = word.to_lowercase().replace('-', "_");
        let n = self.name.to_lowercase().replace('-', "_");
        w == n || w == self.code_name().to_lowercase() || n.rsplit('/').next() == Some(w.as_str())
    }
}

/// The libraries of a folder: those it names first.
pub fn of(dir: &Path) -> Vec<Dep> {
    let mut out = Vec::new();
    if dir.join("Cargo.toml").exists() {
        out.extend(rust(dir));
    }
    if dir.join("package.json").exists() {
        out.extend(npm(dir));
    }
    if dir.join("go.mod").exists() {
        out.extend(go(dir));
    }
    if ["requirements.txt", "pyproject.toml", "setup.py", "Pipfile"].iter().any(|f| dir.join(f).exists()) {
        out.extend(python(dir));
    }
    out.sort_by_key(|d| !d.direct);
    out
}

/// Only the names a folder's manifests give its libraries (no versions,
/// no folders): whether a query may name one, before reading more.
pub fn names(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(t) = std::fs::read_to_string(dir.join("Cargo.toml")) {
        for (k, n) in cargo_deps(&t) {
            out.push(k);
            out.push(n.package);
        }
    }
    if let Ok(t) = std::fs::read_to_string(dir.join("package.json"))
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&t)
    {
        for table in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"] {
            if let Some(o) = v[table].as_object() {
                out.extend(o.keys().cloned());
            }
        }
    }
    if let Ok(t) = std::fs::read_to_string(dir.join("go.mod")) {
        out.extend(t.lines().filter_map(|l| {
            let l = l.trim().trim_start_matches("require ").trim();
            (l.contains('.') && l.contains('/') && !l.starts_with("module"))
                .then(|| l.split_whitespace().next().unwrap_or("").to_string())
        }));
    }
    out.extend(python_named(dir));
    out
}

// ---- Rust

fn cargo_home() -> PathBuf {
    std::env::var_os("CARGO_HOME").map(PathBuf::from).unwrap_or_else(|| home_dir().join(".cargo"))
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

/// A crate's source in cargo's registry.
fn crate_dir(name: &str, version: &str) -> Option<PathBuf> {
    let src = cargo_home().join("registry").join("src");
    for index in std::fs::read_dir(src).ok()?.flatten() {
        let d = index.path().join(format!("{name}-{version}"));
        if d.join("Cargo.toml").is_file() {
            return Some(d);
        }
    }
    None
}

/// The dependency tables of a Cargo.toml, line by line: what it names,
/// with the package each name stands for and the features asked for.
struct Named {
    package: String,
    features: Vec<String>,
    default_features: bool,
}

fn cargo_deps(toml: &str) -> HashMap<String, Named> {
    let mut out = HashMap::new();
    let mut section = String::new();
    // [dependencies.name] tables
    let mut table: Option<String> = None;
    for raw in toml.lines() {
        let line = raw.split(" #").next().unwrap_or(raw).trim();
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').trim().to_string();
            table = None;
            for key in ["dependencies.", "dev-dependencies.", "build-dependencies."] {
                if let Some(i) = section.find(key) {
                    let name = section[i + key.len()..].trim_matches('"').to_string();
                    out.entry(name.clone()).or_insert(Named {
                        package: name.clone(),
                        features: Vec::new(),
                        default_features: true,
                    });
                    table = Some(name);
                }
            }
            continue;
        }
        if let Some(name) = &table {
            if let Some((k, v)) = line.split_once('=') {
                let e = out.get_mut(name).expect("entry");
                match k.trim() {
                    "package" => e.package = v.trim().trim_matches('"').to_string(),
                    "features" => e.features = list(v),
                    "default-features" | "default_features" => e.default_features = v.trim() != "false",
                    _ => {}
                }
            }
            continue;
        }
        let is_deps = section == "dependencies"
            || section == "dev-dependencies"
            || section == "build-dependencies"
            || section == "workspace.dependencies"
            || (section.starts_with("target.") && section.ends_with("dependencies"));
        if !is_deps {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let name = k.trim().trim_matches('"').to_string();
        if name.is_empty() || name.contains(' ') {
            continue;
        }
        let v = v.trim();
        let mut n = Named { package: name.clone(), features: Vec::new(), default_features: true };
        if v.starts_with('{') {
            if let Some(p) = field(v, "package") {
                n.package = p;
            }
            if let Some(i) = v.find("features") {
                let rest = &v[i..];
                if let (Some(a), Some(b)) = (rest.find('['), rest.find(']')) {
                    n.features = list(&rest[a..=b]);
                }
            }
            n.default_features = !v.contains("default-features = false") && !v.contains("default_features = false");
        }
        out.insert(name, n);
    }
    out
}

/// `key = "value"` inside an inline table.
fn field(table: &str, key: &str) -> Option<String> {
    let i = table.find(&format!("{key} ="))?;
    let rest = &table[i..];
    let a = rest.find('"')?;
    let b = rest[a + 1..].find('"')?;
    Some(rest[a + 1..a + 1 + b].to_string())
}

/// `["a", "b"]` as its strings.
fn list(v: &str) -> Vec<String> {
    v.split(['[', ']', ',']).map(|s| s.trim().trim_matches('"').to_string()).filter(|s| !s.is_empty()).collect()
}

/// The versions in a Cargo.lock: name, version, from a registry.
fn cargo_lock(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let (mut name, mut version, mut registry) = (String::new(), String::new(), false);
    let mut flush = |name: &mut String, version: &mut String, registry: &mut bool| {
        if !name.is_empty() && !version.is_empty() && *registry {
            out.push((name.clone(), version.clone()));
        }
        name.clear();
        version.clear();
        *registry = false;
    };
    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            flush(&mut name, &mut version, &mut registry);
        } else if let Some(v) = line.strip_prefix("name = ") {
            name = v.trim_matches('"').to_string();
        } else if let Some(v) = line.strip_prefix("version = ") {
            version = v.trim_matches('"').to_string();
        } else if let Some(v) = line.strip_prefix("source = ") {
            registry = v.contains("registry+");
        }
    }
    flush(&mut name, &mut version, &mut registry);
    out
}

/// The highest of versions written like 1.0.154.
fn highest<'a>(vs: impl Iterator<Item = &'a String>) -> Option<&'a String> {
    vs.max_by_key(|v| v.split(['.', '-', '+']).map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>())
}

fn rust(dir: &Path) -> Vec<Dep> {
    let Ok(toml) = std::fs::read_to_string(dir.join("Cargo.toml")) else { return Vec::new() };
    // the lock of a workspace is at its root
    let lock = dir.ancestors().take(4).map(|d| d.join("Cargo.lock")).find(|p| p.is_file());
    let Some(lock) = lock.and_then(|p| std::fs::read_to_string(p).ok()) else { return Vec::new() };
    let locked = cargo_lock(&lock);
    let named = cargo_deps(&toml);
    let mut by_name: HashMap<&str, Vec<&String>> = HashMap::new();
    for (n, v) in &locked {
        by_name.entry(n.as_str()).or_default().push(v);
    }
    let mut out = Vec::new();
    for n in named.values() {
        let Some(version) = by_name.get(n.package.as_str()).and_then(|vs| highest(vs.iter().copied())) else {
            continue;
        };
        let dir = crate_dir(&n.package, version);
        let features = dir.as_ref().map(|_| (n.features.clone(), n.default_features));
        out.push(Dep { eco: "rust", name: n.package.clone(), version: version.clone(), dir, direct: true, features });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// A crate's default features, as its Cargo.toml lists them.
fn crate_defaults(dir: &Path) -> Vec<String> {
    let Ok(t) = std::fs::read_to_string(dir.join("Cargo.toml")) else { return Vec::new() };
    let mut in_features = false;
    for line in t.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_features = l == "[features]";
        } else if in_features && l.starts_with("default") && l.contains('=') {
            return list(l.split_once('=').map(|x| x.1).unwrap_or(""));
        }
    }
    Vec::new()
}

// ---- npm

fn npm(dir: &Path) -> Vec<Dep> {
    let Ok(pkg) = std::fs::read_to_string(dir.join("package.json")) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&pkg) else { return Vec::new() };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for table in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"] {
        let Some(deps) = v[table].as_object() else { continue };
        for name in deps.keys() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let at = dir.join("node_modules").join(name);
            let version = std::fs::read_to_string(at.join("package.json"))
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .and_then(|p| p["version"].as_str().map(str::to_string));
            let Some(version) = version else { continue };
            out.push(Dep { eco: "npm", name: name.clone(), version, dir: Some(at), direct: true, features: None });
        }
    }
    out
}

// ---- Go

fn go(dir: &Path) -> Vec<Dep> {
    let Ok(text) = std::fs::read_to_string(dir.join("go.mod")) else { return Vec::new() };
    let cache = std::env::var_os("GOMODCACHE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("GOPATH").map(|p| PathBuf::from(p).join("pkg").join("mod")))
        .unwrap_or_else(|| home_dir().join("go").join("pkg").join("mod"));
    let mut out = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        let l = line.trim();
        let req = if in_block {
            if l == ")" {
                in_block = false;
                continue;
            }
            l
        } else if l == "require (" {
            in_block = true;
            continue;
        } else if let Some(r) = l.strip_prefix("require ") {
            r
        } else {
            continue;
        };
        let mut parts = req.split_whitespace();
        let (Some(module), Some(version)) = (parts.next(), parts.next()) else { continue };
        let direct = !req.contains("// indirect");
        // the cache writes capitals as !lower
        let escaped: String = module
            .chars()
            .flat_map(|c| if c.is_ascii_uppercase() { vec!['!', c.to_ascii_lowercase()] } else { vec![c] })
            .collect();
        let at = cache.join(format!("{escaped}@{version}"));
        let dir = at.is_dir().then_some(at);
        out.push(Dep {
            eco: "go",
            name: module.to_string(),
            version: version.to_string(),
            dir,
            direct,
            features: None,
        });
    }
    out
}

// ---- Python

fn python(dir: &Path) -> Vec<Dep> {
    let Some(site) = site_packages(dir) else { return Vec::new() };
    let named: HashSet<String> = python_named(dir);
    let mut out = Vec::new();
    for e in std::fs::read_dir(&site).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        let Some(stem) = n.strip_suffix(".dist-info") else { continue };
        let Some((name, version)) = stem.rsplit_once('-') else { continue };
        let norm = name.to_lowercase().replace(['-', '.'], "_");
        if !named.contains(&norm) {
            continue;
        }
        // the module's folder: top_level.txt, else the name
        let top = std::fs::read_to_string(e.path().join("top_level.txt"))
            .ok()
            .and_then(|t| t.lines().next().map(|l| l.trim().to_string()))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| norm.clone());
        let module = site.join(&top);
        let file = site.join(format!("{top}.py"));
        let dir = if module.is_dir() { Some(module) } else { file.is_file().then_some(file) };
        out.push(Dep {
            eco: "python",
            name: name.to_string(),
            version: version.to_string(),
            dir,
            direct: true,
            features: None,
        });
    }
    out
}

fn site_packages(dir: &Path) -> Option<PathBuf> {
    for env in [".venv", "venv", "env", ".env"] {
        let base = dir.join(env);
        let win = base.join("Lib").join("site-packages");
        if win.is_dir() {
            return Some(win);
        }
        let lib = base.join("lib");
        for e in std::fs::read_dir(&lib).into_iter().flatten().flatten() {
            let p = e.path().join("site-packages");
            if e.file_name().to_string_lossy().starts_with("python") && p.is_dir() {
                return Some(p);
            }
        }
    }
    None
}

/// The packages a Python project names (requirements.txt, pyproject.toml).
fn python_named(dir: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut add = |s: &str| {
        let name: String =
            s.trim().chars().take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.')).collect();
        if !name.is_empty() {
            out.insert(name.to_lowercase().replace(['-', '.'], "_"));
        }
    };
    if let Ok(t) = std::fs::read_to_string(dir.join("requirements.txt")) {
        t.lines().filter(|l| !l.trim_start().starts_with(['#', '-'])).for_each(&mut add);
    }
    if let Ok(t) = std::fs::read_to_string(dir.join("pyproject.toml")) {
        // dependencies = ["requests>=2", ...] and [tool.poetry.dependencies] keys
        let mut in_poetry = false;
        for line in t.lines() {
            let l = line.trim();
            if l.starts_with('[') {
                in_poetry = l.contains("dependencies");
                continue;
            }
            if l.starts_with('"') {
                add(l.trim_matches(|c| c == '"' || c == ','));
            } else if in_poetry && let Some((k, _)) = l.split_once('=') {
                add(k);
            }
        }
        if let (Some(a), Some(b)) =
            (t.find("dependencies = ["), t.find("dependencies = [").and_then(|i| t[i..].find(']').map(|j| i + j)))
        {
            for item in t[a + 16..b].split(',') {
                add(item.trim().trim_matches('"').trim_matches('\''));
            }
        }
    }
    out.remove("python");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_tables_and_the_lock() {
        let toml = r#"
[package]
name = "app"

[dependencies]
serde = { version = "1", features = ["derive"] }
json = { package = "serde_json", version = "1" }
tree-sitter = "0.27.1"
quiet = { version = "2", default-features = false }

[dev-dependencies.tempfile]
version = "3"
"#;
        let d = cargo_deps(toml);
        assert_eq!(d["serde"].features, ["derive"]);
        assert_eq!(d["json"].package, "serde_json");
        assert!(d.contains_key("tree-sitter") && d.contains_key("tempfile"));
        assert!(!d["quiet"].default_features);
        let lock = "[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.200\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.9\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";
        let l = cargo_lock(lock);
        assert_eq!(l.len(), 2, "the app itself is not from a registry");
        let vs: Vec<&String> = l.iter().map(|x| &x.1).collect();
        assert_eq!(highest(vs.into_iter()).unwrap(), "1.0.200");
    }

    #[test]
    fn names_as_code_writes_them() {
        let d = |eco, name: &str| Dep {
            eco,
            name: name.into(),
            version: "1".into(),
            dir: None,
            direct: true,
            features: None,
        };
        assert_eq!(d("rust", "tree-sitter").code_name(), "tree_sitter");
        assert!(d("rust", "tree-sitter").named_by("tree_sitter") && d("rust", "tree-sitter").named_by("Tree-Sitter"));
        assert_eq!(d("go", "github.com/spf13/cobra").code_name(), "cobra");
        assert!(d("go", "github.com/spf13/cobra").named_by("cobra"));
        assert!(!d("npm", "react").named_by("reactive"));
    }
}
