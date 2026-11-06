//! A library's public API, read from its source with the same parsers as
//! the project's code: each item's public path (tree_sitter::Parser::
//! set_language), full signature, first lines of its doc, an example from
//! its doc, the feature it needs (Rust) and whether it is deprecated.

use super::deps::Dep;
use crate::code::lang::Lang;
use crate::code::parse;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// A library this large is not read (windows, aws-sdk): its docs are better.
const MAX_FILES: usize = 1500;
const MAX_BYTES: u64 = 25_000_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    /// How code names it: tree_sitter::Parser::set_language, lodash.debounce.
    pub path: String,
    pub name: String,
    pub kind: String,
    pub signature: String,
    pub doc: String,
    pub example: String,
    /// Rust: the feature it needs.
    pub feature: String,
    /// What the library says instead, when it is deprecated ("deprecated" when it says nothing).
    pub deprecated: String,
    /// Its file, below the library's folder, and lines.
    pub file: String,
    pub line: usize,
    pub end: usize,
}

/// Why a library was not read.
#[derive(Debug, PartialEq)]
pub enum Skip {
    NotHere,
    TooBig(usize),
}

/// The library's public API.
pub fn items(dep: &Dep) -> Result<Vec<Item>, Skip> {
    let Some(dir) = &dep.dir else { return Err(Skip::NotHere) };
    let (root, files) = match dep.eco {
        "rust" => rust_files(dir),
        "npm" => npm_files(dir, &dep.name),
        "go" => (
            dir.clone(),
            walk(
                dir,
                &|p| p.ends_with(".go") && !p.ends_with("_test.go"),
                &["testdata", "internal", "vendor", "examples", "example"],
            ),
        ),
        "python" => {
            if dir.is_file() {
                (dir.parent().unwrap_or(dir).to_path_buf(), vec![dir.clone()])
            } else {
                (
                    dir.parent().unwrap_or(dir).to_path_buf(),
                    walk(
                        dir,
                        &|p| p.ends_with(".py") || p.ends_with(".pyi"),
                        &["tests", "test", "testing", "__pycache__"],
                    ),
                )
            }
        }
        _ => return Err(Skip::NotHere),
    };
    let bytes: u64 = files.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
    if files.len() > MAX_FILES || bytes > MAX_BYTES {
        return Err(Skip::TooBig(files.len()));
    }
    let mut out = Vec::new();
    // Rust: an item of a private module is the crate's own, unless re-exported
    let mut private: Vec<bool> = Vec::new();
    let modules = if dep.eco == "rust" { rust_modules(&root, &files, &dep.code_name()) } else { HashMap::new() };
    let gated = if dep.eco == "rust" { gated_modules(&root, &files, &dep.code_name()) } else { HashMap::new() };
    for f in &files {
        let Ok(src) = std::fs::read_to_string(f) else { continue };
        let rel = f.strip_prefix(dir).unwrap_or(f).to_string_lossy().replace('\\', "/");
        let module = module_of(dep, &root, f);
        let Some(lang) = Lang::of(&f.to_string_lossy()) else { continue };
        let lines: Vec<&str> = src.lines().collect();
        let symbols = parse::symbols(lang, &src);
        let hidden = dep.eco == "rust" && !module_public(&module, &dep.code_name(), &modules);
        for s in &symbols {
            if !public(dep.eco, s, &symbols) {
                continue;
            }
            let mut item = Item {
                path: join(dep.eco, &module, &s.qualified),
                name: s.name.clone(),
                kind: s.kind.to_string(),
                signature: signature(&lines, s.start, dep.eco),
                file: rel.clone(),
                line: s.start,
                end: s.end,
                ..Item::default()
            };
            about(dep.eco, &lines, s.start, &mut item);
            if item.feature.is_empty()
                && let Some(f) = gated.get(&module)
            {
                item.feature = f.clone();
            }
            out.push(item);
            private.push(hidden);
        }
        if dep.eco == "rust" {
            let exported = re_exports(&src, &module, &rel);
            private.extend(exported.iter().map(|_| hidden));
            out.extend(exported);
        }
    }
    let members = resolve_re_exports(&mut out);
    private.extend(members.iter().map(|_| false));
    out.extend(members);
    if dep.eco == "rust" {
        // impl Connection in a private module: still Connection's methods
        let mut types: HashMap<String, Vec<String>> = HashMap::new();
        for (i, p) in out.iter().zip(&private) {
            if !p && matches!(i.kind.as_str(), "struct" | "enum" | "trait" | "type" | "union") {
                types.entry(i.name.clone()).or_default().push(i.path.clone());
            }
        }
        let mut moved = Vec::new();
        for (i, p) in out.iter().zip(&private) {
            let parts: Vec<&str> = i.path.split("::").collect();
            if *p && i.kind == "method" && parts.len() >= 3 {
                let ty = parts[parts.len() - 2];
                if let Some(at) = types.get(ty).and_then(|v| v.iter().min_by_key(|p| p.len())) {
                    moved.push(Item { path: format!("{at}::{}", i.name), ..i.clone() });
                }
            }
        }
        private.extend(moved.iter().map(|_| false));
        out.extend(moved);
    }
    let mut keep = private.into_iter().map(|p| !p);
    out.retain(|i| keep.next().unwrap_or(true) && i.kind != "glob");
    // one item reached two ways (a glob and a re-export): once
    let mut seen = std::collections::HashSet::new();
    out.retain(|i| seen.insert(i.path.clone()));
    Ok(out)
}

static MOD_DECL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*(pub(?:\([^)]*\))?[ \t]+)?mod[ \t]+(\w+)[ \t]*[;{]").expect("regex"));

/// The modules a crate declares: their path, and whether they are public.
fn rust_modules(root: &Path, files: &[PathBuf], crate_name: &str) -> HashMap<String, bool> {
    let mut out = HashMap::new();
    for f in files {
        let Ok(src) = std::fs::read_to_string(f) else { continue };
        let dep = Dep {
            eco: "rust",
            name: crate_name.to_string(),
            version: String::new(),
            dir: None,
            direct: true,
            features: None,
        };
        let parent = module_of(&dep, root, f);
        for c in MOD_DECL.captures_iter(&src) {
            let public = c.get(1).is_some_and(|v| v.as_str().trim() == "pub");
            out.insert(format!("{parent}::{}", &c[2]), public);
        }
    }
    out
}

/// Whether code outside the crate reaches a module: each module on its
/// way declared `pub mod`. One no file declares (a file include!d into
/// another) is not reached.
fn module_public(module: &str, crate_name: &str, modules: &HashMap<String, bool>) -> bool {
    let mut at = crate_name.to_string();
    for p in module.split("::").skip(1) {
        at = format!("{at}::{p}");
        if modules.get(&at) != Some(&true) {
            return false;
        }
    }
    true
}

/// The crate's source folder and its .rs files (not tests, benches, examples).
fn rust_files(dir: &Path) -> (PathBuf, Vec<PathBuf>) {
    let lib = std::fs::read_to_string(dir.join("Cargo.toml"))
        .ok()
        .and_then(|t| {
            let i = t.find("[lib]")?;
            let rest = &t[i..];
            let end = rest[1..].find("\n[").map(|e| e + 1).unwrap_or(rest.len());
            rest[..end].lines().find_map(|l| {
                l.trim()
                    .strip_prefix("path")
                    .map(|v| v.trim_start_matches([' ', '=']).trim().trim_matches('"').to_string())
            })
        })
        .unwrap_or_else(|| "src/lib.rs".to_string());
    let root = dir.join(&lib).parent().map(Path::to_path_buf).unwrap_or_else(|| dir.join("src"));
    let files = walk(&root, &|p| p.ends_with(".rs") && !p.ends_with("build.rs"), &["tests", "benches", "examples"]);
    (root, files)
}

/// A package's type declarations: its `types`, else every .d.ts in it,
/// else @types/<name>, else its JavaScript.
fn npm_files(dir: &Path, name: &str) -> (PathBuf, Vec<PathBuf>) {
    let dts = walk(
        dir,
        &|p| p.ends_with(".d.ts") || p.ends_with(".d.mts") || p.ends_with(".d.cts"),
        &["node_modules", "test", "tests", "__tests__"],
    );
    if !dts.is_empty() {
        return (dir.to_path_buf(), dts);
    }
    let types = dir
        .ancestors()
        .nth(if name.contains('/') { 2 } else { 1 })
        .map(|nm| nm.join("@types").join(name.replace('/', "__").trim_start_matches('@')));
    if let Some(t) = types.filter(|t| t.is_dir()) {
        let found = walk(&t, &|p| p.ends_with(".d.ts"), &["node_modules"]);
        if !found.is_empty() {
            return (t, found);
        }
    }
    let js = walk(
        dir,
        &|p| p.ends_with(".js") || p.ends_with(".mjs") || p.ends_with(".cjs"),
        &["node_modules", "test", "tests", "__tests__", "dist/esm", "umd"],
    );
    (dir.to_path_buf(), js.into_iter().take(200).collect())
}

/// Files below `dir` that `keep` takes, leaving out folders named `skip`.
fn walk(dir: &Path, keep: &dyn Fn(&str) -> bool, skip: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                if !name.starts_with('.') && !skip.contains(&name.as_str()) {
                    stack.push(p);
                }
            } else if keep(&name) {
                out.push(p);
            }
            if out.len() > MAX_FILES {
                return out;
            }
        }
    }
    out.sort();
    out
}

/// The module a file is: tree_sitter::query, cobra, requests.adapters.
fn module_of(dep: &Dep, root: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/");
    let mut parts: Vec<&str> = rel.split('/').collect();
    let last = parts.pop().unwrap_or("");
    let stem = last.split('.').next().unwrap_or(last);
    match dep.eco {
        "rust" => {
            if !matches!(stem, "lib" | "mod" | "main") {
                parts.push(stem);
            }
            std::iter::once(dep.code_name().as_str()).chain(parts).collect::<Vec<_>>().join("::")
        }
        "go" => {
            // the package is its folder
            parts.last().map(|p| p.to_string()).unwrap_or_else(|| dep.code_name())
        }
        "python" => {
            if stem != "__init__" {
                parts.push(stem);
            }
            parts.join(".")
        }
        _ => dep.name.clone(),
    }
}

fn join(eco: &str, module: &str, qualified: &str) -> String {
    // "Parser Default.default": a trait's method on Parser is Parser::default
    // and Map<String, Value>.get is Map::get: no generics in a path
    let qualified = qualified
        .split('.')
        .map(|p| {
            let p = p.split_whitespace().next().unwrap_or(p);
            p.split('<').next().unwrap_or(p)
        })
        .collect::<Vec<_>>()
        .join(".");
    if eco == "rust" {
        format!("{module}::{}", qualified.replace('.', "::"))
    } else if module.is_empty() {
        qualified.to_string()
    } else {
        format!("{module}.{qualified}")
    }
}

/// Whether code outside the library can use it.
fn public(eco: &str, s: &parse::Symbol, all: &[parse::Symbol]) -> bool {
    if matches!(s.kind, "impl" | "module" | "section") {
        return false;
    }
    match eco {
        "rust" => {
            let sig = s.signature.trim_start();
            let restricted = ["pub(crate)", "pub(super)", "pub(in ", "pub(self)"].iter().any(|r| sig.starts_with(r));
            if sig.starts_with("pub") && !restricted {
                return true;
            }
            // a trait's method, in the trait or in an implementation of it for
            // a type: no pub of its own
            s.kind == "method"
                && all.iter().any(|i| {
                    i.start <= s.start
                        && s.end <= i.end
                        && ((i.kind == "impl" && i.signature.contains(" for "))
                            || (i.kind == "trait" && i.signature.trim_start().starts_with("pub")))
                })
        }
        "go" => s.qualified.split('.').all(|p| p.chars().next().is_some_and(char::is_uppercase)),
        "python" => s.qualified.split('.').all(|p| !p.starts_with('_')),
        _ => !s.name.starts_with('_'),
    }
}

/// The definition up to its body, on one line.
fn signature(lines: &[&str], start: usize, eco: &str) -> String {
    let mut out = String::new();
    for l in lines.iter().skip(start.saturating_sub(1)).take(10) {
        let l = l.trim();
        out.push_str(l);
        out.push(' ');
        let ends = match eco {
            "python" => l.ends_with(':'),
            _ => l.contains('{') || l.ends_with(';') || l.ends_with(')') && eco == "npm",
        };
        if ends {
            break;
        }
    }
    let mut s = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(i) = s.find(" {") {
        s.truncate(i);
    }
    if (s.contains("const ") || s.contains("static "))
        && !s.contains("fn ")
        && let Some(i) = s.find(" = ")
    {
        s.truncate(i);
    }
    s.trim_end_matches(['{', ';', ':']).trim().to_string()
}

static FEATURE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"feature\s*=\s*"([^"]+)""#).expect("regex"));
static NOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"note\s*=\s*"([^"]+)""#).expect("regex"));

/// Its doc, example, feature and deprecation, from the lines above it
/// (and, for Python, its docstring below).
fn about(eco: &str, lines: &[&str], start: usize, item: &mut Item) {
    let mut doc: Vec<String> = Vec::new();
    let mut i = start.saturating_sub(1);
    // above: doc comments and attributes, nearest first
    while i > 0 {
        let l = lines[i - 1].trim();
        let is_doc = match eco {
            "rust" => {
                l.starts_with("///")
                    || l.starts_with("#[")
                    || l.starts_with("#![")
                    || l.ends_with(")]")
                    || l.ends_with("]")
            }
            "go" => l.starts_with("//"),
            "python" => l.starts_with('@') || l.starts_with('#'),
            _ => {
                l.starts_with("/**")
                    || l.starts_with('*')
                    || l.starts_with("*/")
                    || l.starts_with("//")
                    || l.starts_with('@')
            }
        };
        if !is_doc || l.is_empty() {
            break;
        }
        if l.starts_with("#[") || l.starts_with('@') && eco == "python" || l.ends_with(")]") {
            if let Some(f) = FEATURE.captures(l).filter(|_| l.contains("cfg")) {
                item.feature = f[1].to_string();
            }
            if l.contains("deprecated") {
                item.deprecated = NOTE.captures(l).map(|c| c[1].to_string()).unwrap_or_else(|| "deprecated".into());
            }
        } else {
            let text = l.trim_start_matches(['/', '*']).trim_start_matches('!').trim();
            doc.push(text.to_string());
        }
        i -= 1;
    }
    doc.reverse();
    if eco == "python" {
        doc.extend(docstring(lines, start));
    }
    // JSDoc and Go put deprecation in the doc
    for d in &doc {
        if let Some(rest) = d.strip_prefix("@deprecated").or_else(|| d.strip_prefix("Deprecated:")) {
            item.deprecated = if rest.trim().is_empty() { "deprecated".into() } else { rest.trim().to_string() };
        }
    }
    // the example: a fenced block, @example lines, or >>> lines
    let mut ex = Vec::new();
    let mut fenced = false;
    for d in &doc {
        if d.starts_with("```") {
            if fenced {
                break;
            }
            fenced = true;
            continue;
        }
        if fenced && !d.starts_with("# ") && d != "#" {
            ex.push(d.clone());
        } else if let Some(code) = d.strip_prefix(">>> ") {
            ex.push(code.to_string());
        }
        if ex.len() >= 6 {
            break;
        }
    }
    item.example = ex.join("\n");
    // the first paragraph
    let mut first = Vec::new();
    for d in doc.iter().map(|d| d.trim()) {
        if d.is_empty() && !first.is_empty() || d.starts_with('#') || d.starts_with("```") || d.starts_with('@') {
            break;
        }
        if !d.is_empty() {
            first.push(d);
        }
    }
    let mut text = first.join(" ");
    if text.chars().count() > 300 {
        text = text.chars().take(297).collect::<String>() + "...";
    }
    item.doc = text;
}

/// A Python function's docstring, below its definition line.
fn docstring(lines: &[&str], start: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut open = false;
    for l in lines.iter().skip(start).take(40) {
        let t = l.trim();
        if !open {
            if t.starts_with("\"\"\"") || t.starts_with("'''") {
                let rest = &t[3..];
                if let Some(end) = rest.find("\"\"\"").or_else(|| rest.find("'''")) {
                    out.push(rest[..end].to_string());
                    return out;
                }
                out.push(rest.to_string());
                open = true;
                continue;
            }
            if !t.is_empty() {
                return out;
            }
        } else {
            if let Some(end) = t.find("\"\"\"").or_else(|| t.find("'''")) {
                out.push(t[..end].to_string());
                return out;
            }
            out.push(t.to_string());
        }
    }
    out
}

static MOD_GATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"#\[cfg(?:_attr)?\([^\]]*feature\s*=\s*"([^"]+)"[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*pub\s+mod\s+(\w+)"#)
        .expect("regex")
});

/// Modules a feature turns on: `#[cfg(feature = "x")] pub mod name;`.
fn gated_modules(root: &Path, files: &[PathBuf], crate_name: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for f in files {
        let Ok(src) = std::fs::read_to_string(f) else { continue };
        if !src.contains("feature") {
            continue;
        }
        let dep = Dep {
            eco: "rust",
            name: crate_name.to_string(),
            version: String::new(),
            dir: None,
            direct: true,
            features: None,
        };
        let module = module_of(&dep, root, f);
        for c in MOD_GATE.captures_iter(&src) {
            out.insert(format!("{module}::{}", &c[2]), c[1].to_string());
        }
    }
    out
}

static PUB_USE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*((?:#\[[^\]]*\][ \t]*\n?[ \t]*)*)pub use ([\w:]+?)(?:::\{([^}]*)\})?(?: as (\w+))?;")
        .expect("regex")
});

/// `pub use` in a module: the names it makes public there, pointing at
/// what they stand for (resolved later).
fn re_exports(src: &str, module: &str, rel: &str) -> Vec<Item> {
    let mut out = Vec::new();
    for c in PUB_USE.captures_iter(src) {
        let attrs = c.get(1).map(|m| m.as_str()).unwrap_or("");
        let feature =
            FEATURE.captures(attrs).filter(|_| attrs.contains("cfg")).map(|f| f[1].to_string()).unwrap_or_default();
        let base = &c[2];
        let line = src[..c.get(0).map(|m| m.start()).unwrap_or(0)].lines().count() + 1;
        let mut push = |target: String, alias: Option<&str>| {
            let name =
                alias.map(str::to_string).unwrap_or_else(|| target.rsplit("::").next().unwrap_or(&target).to_string());
            if name == "*" {
                // everything public of a module, here: resolved with the rest
                out.push(Item {
                    path: module.to_string(),
                    kind: "glob".into(),
                    signature: format!("pub use {target}"),
                    feature: feature.clone(),
                    file: rel.to_string(),
                    line,
                    end: line,
                    ..Item::default()
                });
                return;
            }
            if name == "self" || name.is_empty() {
                return;
            }
            out.push(Item {
                path: format!("{module}::{name}"),
                name,
                kind: "re-export".into(),
                signature: format!("pub use {target}"),
                feature: feature.clone(),
                file: rel.to_string(),
                line,
                end: line,
                ..Item::default()
            });
        };
        match c.get(3) {
            Some(list) => {
                for part in list.as_str().split(',') {
                    let part = part.trim();
                    let (what, alias) = match part.split_once(" as ") {
                        Some((w, a)) => (w.trim(), Some(a.trim())),
                        None => (part, None),
                    };
                    if !what.is_empty() {
                        push(format!("{base}::{what}"), alias);
                    }
                }
            }
            None => push(base.to_string(), c.get(4).map(|m| m.as_str())),
        }
    }
    out
}

/// A re-export takes what it stands for (signature, doc...) when that is
/// in the library; one of another crate stays as `pub use`. A type's
/// methods come with it, under its new path: returned to be added.
fn resolve_re_exports(items: &mut [Item]) -> Vec<Item> {
    let by_name: HashMap<String, Vec<usize>> =
        items.iter().enumerate().filter(|(_, i)| i.kind != "re-export").fold(HashMap::new(), |mut m, (n, i)| {
            m.entry(i.name.clone()).or_default().push(n);
            m
        });
    let found: Vec<(usize, usize)> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.kind == "re-export")
        .filter_map(|(n, i)| {
            let target = i.signature.trim_start_matches("pub use ");
            let tail = target.rsplit("::").next().unwrap_or(target);
            let rest = target.trim_start_matches("crate::").trim_start_matches("self::").trim_start_matches("super::");
            by_name
                .get(tail)?
                .iter()
                .find(|&&k| items[k].path.ends_with(rest) || items[k].path.ends_with(&format!("::{tail}")))
                .map(|&k| (n, k))
        })
        .collect();
    let mut members = Vec::new();
    // globs: what the module they name holds, under the module they are in
    for g in items.iter().filter(|i| i.kind == "glob") {
        let target = g.signature.trim_start_matches("pub use ").trim_end_matches("::*");
        let crate_name = g.path.split("::").next().unwrap_or("");
        let parent = g.path.rsplit_once("::").map(|p| p.0).unwrap_or(crate_name);
        let candidates = if let Some(rest) = target.strip_prefix("crate::") {
            vec![format!("{crate_name}::{rest}")]
        } else if let Some(rest) = target.strip_prefix("self::") {
            vec![format!("{}::{rest}", g.path)]
        } else if let Some(rest) = target.strip_prefix("super::") {
            vec![format!("{parent}::{rest}")]
        } else {
            vec![format!("{}::{target}", g.path), format!("{crate_name}::{target}")]
        };
        for from in candidates {
            let prefix = format!("{from}::");
            for m in
                items.iter().filter(|m| m.path.starts_with(&prefix) && !matches!(m.kind.as_str(), "re-export" | "glob"))
            {
                let mut m = m.clone();
                m.path = format!("{}::{}", g.path, &m.path[prefix.len()..]);
                if m.feature.is_empty() {
                    m.feature = g.feature.clone();
                }
                members.push(m);
            }
        }
    }
    for (n, k) in found {
        let (path, name, feature) = (items[n].path.clone(), items[n].name.clone(), items[n].feature.clone());
        let mut it = items[k].clone();
        let was = format!("{}::", it.path);
        it.path = path.clone();
        it.name = name;
        if it.feature.is_empty() {
            it.feature = feature.clone();
        }
        items[n] = it;
        for m in items.iter().filter(|m| m.path.starts_with(&was) && m.kind != "re-export") {
            let mut m = m.clone();
            m.path = format!("{path}::{}", &m.path[was.len()..]);
            if m.feature.is_empty() {
                m.feature = feature.clone();
            }
            members.push(m);
        }
    }
    members
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crate_at(files: &[(&str, &str)]) -> (tempfile::TempDir, Dep) {
        let d = tempfile::tempdir().unwrap();
        for (p, s) in files {
            let f = d.path().join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, s).unwrap();
        }
        let dep = Dep {
            eco: "rust",
            name: "demo-lib".into(),
            version: "1.2.3".into(),
            dir: Some(d.path().to_path_buf()),
            direct: true,
            features: None,
        };
        (d, dep)
    }

    #[test]
    fn a_crates_public_api_with_its_doc_example_feature_and_deprecation() {
        let (_d, dep) = crate_at(&[
            ("Cargo.toml", "[package]\nname = \"demo-lib\"\n"),
            (
                "src/lib.rs",
                "//! Demo.\n\nmod inner;\nmod cache;\npub use cache::Context;\nmod strings;\npub use crate::{strings::string::*};\n#[cfg(feature = \"json\")]\npub mod json;\npub use inner::Parser;\n#[cfg(feature = \"derive\")]\npub use demo_derive::{Serialize, Deserialize as De};\n\n/// The language of a grammar.\npub const LANGUAGE: u32 = 1;\n\nfn private_helper() {}\n\n/// Old way.\n#[deprecated(since = \"1.1\", note = \"use LANGUAGE\")]\npub fn language() -> u32 {\n    LANGUAGE\n}\n",
            ),
            (
                "src/inner.rs",
                "/// Parses text.\n///\n/// ```\n/// let mut p = demo_lib::Parser::new();\n/// p.set_language(1)?;\n/// ```\npub struct Parser;\n\nimpl Parser {\n    /// Makes one.\n    pub fn new() -> Self {\n        Parser\n    }\n\n    /// Sets the language,\n    /// before parsing.\n    pub fn set_language(\n        &mut self,\n        language: u32,\n    ) -> Result<(), String> {\n        Ok(())\n    }\n\n    pub(crate) fn hidden(&self) {}\n}\n\nimpl Default for Parser {\n    fn default() -> Self {\n        Parser\n    }\n}\n",
            ),
            ("src/json.rs", "/// To JSON.\npub fn to_string(v: u32) -> String {\n    v.to_string()\n}\n"),
            (
                "src/cache.rs",
                "use crate::Parser;\n\nimpl Parser {\n    /// Kept for the next time.\n    pub fn prepare_cached(&self) {}\n}\n\n/// Adds context.\npub trait Context {\n    /// With a closure.\n    fn with_context(self) -> Self;\n}\n",
            ),
            ("src/strings/mod.rs", "pub mod string;\n"),
            (
                "src/strings/string.rs",
                "/// A pattern.\npub struct Regex;\n\nimpl Regex {\n    /// All matches.\n    pub fn captures_iter(&self) -> u32 {\n        0\n    }\n}\n",
            ),
        ]);
        let items = items(&dep).unwrap();
        let get = |p: &str| {
            items
                .iter()
                .find(|i| i.path == p)
                .unwrap_or_else(|| panic!("{p} not in {:#?}", items.iter().map(|i| &i.path).collect::<Vec<_>>()))
        };
        let set = get("demo_lib::Parser::set_language");
        assert_eq!(set.signature, "pub fn set_language( &mut self, language: u32, ) -> Result<(), String>");
        assert_eq!(set.doc, "Sets the language, before parsing.");
        // the re-export at the root, with what it stands for
        let parser = get("demo_lib::Parser");
        assert_eq!(parser.doc, "Parses text.");
        assert!(parser.example.contains("p.set_language(1)?;"), "{parser:?}");
        // gated by a feature: a module, and re-exports of another crate
        assert_eq!(get("demo_lib::json::to_string").feature, "json");
        assert_eq!(get("demo_lib::Serialize").feature, "derive");
        assert_eq!(get("demo_lib::De").signature, "pub use demo_derive::Deserialize");
        assert_eq!(get("demo_lib::language").deprecated, "use LANGUAGE");
        assert_eq!(get("demo_lib::LANGUAGE").signature, "pub const LANGUAGE: u32");
        // a glob from a private module: its items (and their methods) at the root
        assert_eq!(get("demo_lib::Regex::captures_iter").doc, "All matches.");
        assert!(!items.iter().any(|i| i.path.starts_with("demo_lib::strings::")));
        assert_eq!(get("demo_lib::Parser::prepare_cached").doc, "Kept for the next time.");
        assert_eq!(get("demo_lib::Context::with_context").signature, "fn with_context(self) -> Self");
        assert!(get("demo_lib::LANGUAGE").deprecated.is_empty());
        // a trait's method is public without pub; private ones are not
        assert!(items.iter().all(|i| !i.path.contains("Default")));
        assert!(!items.iter().any(|i| i.name == "hidden" || i.name == "private_helper"));
        // inner is a private module: its items are reached only through the re-export
        assert!(
            !items.iter().any(|i| i.path.starts_with("demo_lib::inner::")),
            "{:?}",
            items.iter().map(|i| &i.path).collect::<Vec<_>>()
        );
    }

    #[test]
    fn too_big_or_not_here() {
        let dep = Dep { eco: "rust", name: "x".into(), version: "1".into(), dir: None, direct: true, features: None };
        assert_eq!(items(&dep), Err(Skip::NotHere));
    }
}
