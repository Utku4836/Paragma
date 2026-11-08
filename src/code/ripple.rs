//! What a change reaches: for each function, method or type whose shape
//! changed since the last commit (its signature, its name, or it is gone),
//! the code that calls it, which of those the same change updated, and the
//! tests that reach it. Read by `pg ripple`, and said by itself in `pg run`,
//! the hooks after an edit, and `pg finish`.
//!
//! Calls are known by name, so a caller is "sure" when its file uses the
//! name through the module it is defined in (an import naming it, the same
//! file or package), else "possible".

use super::lang::Lang;
use super::{CodeIndex, Sym, git, parse};
use anyhow::Result;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

/// How a symbol changed.
#[derive(Clone, Debug, PartialEq)]
pub enum How {
    /// Its definition line is another: parameters, return, generics.
    Signature {
        was: String,
        now: String,
    },
    Renamed {
        to: String,
    },
    Removed,
    /// Only its body: what calls it still fits, its tests should run.
    Body,
    /// A shader struct, a GraphQL or Protobuf type, a SQL table: its fields
    /// (names as they were and are now). Code in other languages that fills
    /// or reads it knows it only by its name and its fields' names.
    Fields {
        removed: Vec<String>,
        added: Vec<String>,
    },
}

/// A caller of a changed symbol.
#[derive(Clone, Debug)]
pub struct Caller {
    pub sym: Sym,
    /// The same change touched it.
    pub updated: bool,
    /// Its file reaches the symbol through its module; else only by name.
    pub sure: bool,
}

#[derive(Clone, Debug)]
pub struct Ripple {
    pub folder: String,
    pub path: String,
    pub name: String,
    pub qualified: String,
    pub line: usize,
    pub how: How,
    pub callers: Vec<Caller>,
    pub tests: Vec<Sym>,
    /// Schemas: other code that names the type or a field it lost, as
    /// file and line (all "possible").
    pub users: Vec<(String, String, usize)>,
    /// A Rust item that is not `pub`: only its own module (this file and
    /// the modules under it) can call it.
    pub private: bool,
}

impl Ripple {
    /// Callers the change left as they were: where it may break.
    pub fn not_updated(&self) -> Vec<&Caller> {
        self.callers.iter().filter(|c| !c.updated).collect()
    }

    /// Whether what calls it may now be wrong.
    pub fn breaks(&self) -> bool {
        match self.how {
            How::Body => false,
            How::Fields { .. } => !self.users.is_empty(),
            _ => self.callers.iter().any(|c| !c.updated),
        }
    }
}

/// Kinds whose shape callers depend on.
fn shaped(kind: &str) -> bool {
    matches!(
        kind,
        "function"
            | "method"
            | "struct"
            | "class"
            | "interface"
            | "trait"
            | "enum"
            | "type"
            | "const"
            | "table"
            | "view"
    )
}

fn norm(sig: &str) -> String {
    sig.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches(['{', ':', ';']).trim().to_string()
}

/// The changes since the last commit that reach other code, in every code
/// folder; `only`: just these files (folder key, path).
pub fn of_changes(code: &CodeIndex, only: Option<&[(String, String)]>) -> Result<Vec<Ripple>> {
    let mut out = Vec::new();
    let mut changed_lines: HashMap<(String, String), Vec<(usize, usize)>> = HashMap::new();
    let mut files: Vec<(String, std::path::PathBuf, String)> = Vec::new();
    for f in &code.folders {
        for (path, ranges) in git::changed(&f.dir) {
            changed_lines
                .insert((f.key.clone(), path.clone()), ranges.clone().unwrap_or_else(|| vec![(1, usize::MAX)]));
            files.push((f.key.clone(), f.dir.clone(), path));
        }
    }
    for (folder, dir, path) in &files {
        if only.is_some_and(|o| !o.iter().any(|(fk, p)| fk == folder && p == path)) {
            continue;
        }
        let Some(lang) = Lang::of(path) else { continue };
        let Some(head) = git::at_head(dir, path) else { continue }; // a new file breaks nothing
        let now = std::fs::read_to_string(dir.join(path)).unwrap_or_default();
        let before = parse::symbols(lang, &head);
        let after = parse::symbols(lang, &now);
        for b in before.iter().filter(|s| shaped(s.kind)) {
            let schema = schema_kind(lang, path, b);
            let how = match after.iter().find(|a| a.qualified == b.qualified && a.kind == b.kind) {
                Some(a) if schema.is_some() && a.body_hash != b.body_hash => {
                    let f = schema.unwrap_or(lang);
                    let was = fields(f, &head, b);
                    let now = fields(f, &now, a);
                    let removed: Vec<String> = was.iter().filter(|x| !now.contains(x)).cloned().collect();
                    let added: Vec<String> = now.iter().filter(|x| !was.contains(x)).cloned().collect();
                    if removed.is_empty() && added.is_empty() { How::Body } else { How::Fields { removed, added } }
                }
                Some(a) if norm(&a.signature) != norm(&b.signature) => {
                    How::Signature { was: norm(&b.signature), now: norm(&a.signature) }
                }
                Some(a) if a.body_hash != b.body_hash => How::Body,
                Some(_) => continue,
                None => match after.iter().find(|a| a.body_hash == b.body_hash && a.kind == b.kind && a.name != b.name)
                {
                    Some(a) => How::Renamed { to: a.qualified.clone() },
                    None => How::Removed,
                },
            };
            // where it is now: under its new name when renamed
            let now_at = match &how {
                How::Renamed { to } => after.iter().find(|a| a.qualified == *to),
                _ => after.iter().find(|a| a.qualified == b.qualified),
            };
            let line = now_at.map(|a| a.start).unwrap_or(b.start);
            let mut r = Ripple {
                folder: folder.clone(),
                path: path.clone(),
                name: b.name.clone(),
                qualified: b.qualified.clone(),
                line,
                how,
                callers: Vec::new(),
                tests: Vec::new(),
                users: Vec::new(),
                private: rust_private(lang, &before, &b.qualified, &b.signature),
            };
            fill(code, &mut r, &changed_lines)?;
            if let How::Fields { removed, .. } = &r.how {
                r.users = users(code, &r, removed, &changed_lines);
            }
            out.push(r);
        }
    }
    // what breaks first, then what only needs its tests run
    out.sort_by_key(|r| (!r.breaks(), matches!(r.how, How::Body)));
    Ok(out)
}

/// What reaches one symbol as it is now (`pg ripple Refund`).
pub fn of_symbol(code: &CodeIndex, s: &Sym) -> Result<Ripple> {
    let private = match Lang::of(&s.path) {
        Some(lang @ Lang::Rust) => {
            let text = code
                .folders
                .iter()
                .find(|f| f.key == s.folder)
                .and_then(|f| std::fs::read_to_string(f.dir.join(&s.path)).ok())
                .unwrap_or_default();
            rust_private(lang, &parse::symbols(lang, &text), &s.qualified, &s.signature)
        }
        _ => false,
    };
    let mut r = Ripple {
        folder: s.folder.clone(),
        path: s.path.clone(),
        name: s.name.clone(),
        qualified: s.qualified.clone(),
        line: s.start,
        how: How::Body,
        callers: Vec::new(),
        tests: Vec::new(),
        users: Vec::new(),
        private,
    };
    let mut lines = HashMap::new();
    for f in &code.folders {
        for (path, ranges) in git::changed(&f.dir) {
            lines.insert((f.key.clone(), path), ranges.unwrap_or_else(|| vec![(1, usize::MAX)]));
        }
    }
    fill(code, &mut r, &lines)?;
    Ok(r)
}

/// Whether a Rust item is not `pub` and so not callable from other modules:
/// not a method of a trait or of a trait's impl, which have no `pub` of
/// their own.
fn rust_private(lang: Lang, syms: &[parse::Symbol], qualified: &str, signature: &str) -> bool {
    if lang != Lang::Rust || signature.trim_start().starts_with("pub") {
        return false;
    }
    match qualified.rsplit_once('.') {
        None => true,
        // "Page<'_>.matching"; a trait impl's is "Money Display.fmt"
        Some((owner, _)) => !owner.contains(' ') && !syms.iter().any(|t| t.kind == "trait" && t.qualified == owner),
    }
}

/// The language of a type's fields, when it is a schema: a shader struct
/// (also one kept in a TypeScript string), a GraphQL or Protobuf type, a
/// SQL table.
fn schema_kind(lang: Lang, path: &str, s: &parse::Symbol) -> Option<Lang> {
    if !matches!(s.kind, "struct" | "type" | "class" | "interface" | "enum")
        && !s.kind.contains("table")
        && !s.kind.contains("message")
    {
        return None;
    }
    match lang {
        Lang::Wgsl | Lang::Glsl | Lang::Hlsl | Lang::GraphQl | Lang::Proto | Lang::Sql => Some(lang),
        // a struct of a shader in a string: SEA_SHADER.Uniforms
        Lang::TypeScript | Lang::JavaScript if s.qualified.contains('.') && path_has_shader(path, s) => {
            Some(Lang::Wgsl)
        }
        _ => None,
    }
}

fn path_has_shader(_path: &str, s: &parse::Symbol) -> bool {
    s.signature.trim_start().starts_with("struct ")
}

static FIELD_COLON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:@\w+(?:\([^)]*\))?\s*)*([A-Za-z_]\w*)\s*[:(]").expect("regex"));
static FIELD_C: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:\w+\s+)*[\w<>]+\s+([A-Za-z_]\w*)\s*(?:\[[^\]]*\])?\s*(?::\s*\w+)?\s*;").expect("regex")
});
static FIELD_PROTO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:repeated\s+|optional\s+|required\s+)?[\w.<>, ]+\s+([A-Za-z_]\w*)\s*=\s*\d+").expect("regex")
});
static FIELD_SQL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*"?([A-Za-z_]\w*)"?\s+[A-Za-z]"#).expect("regex"));

/// The field names of a type, from its lines in `src`.
fn fields(lang: Lang, src: &str, s: &parse::Symbol) -> Vec<String> {
    let lines: Vec<&str> = src.lines().collect();
    let body = lines.iter().skip(s.start).take(s.end.saturating_sub(s.start).saturating_sub(1).max(1));
    let mut out = Vec::new();
    for l in body {
        let l = l.split("//").next().unwrap_or(l).split("--").next().unwrap_or(l);
        let name = match lang {
            Lang::Wgsl | Lang::GraphQl => FIELD_COLON.captures(l).map(|c| c[1].to_string()),
            Lang::Glsl | Lang::Hlsl => FIELD_C.captures(l).map(|c| c[1].to_string()),
            Lang::Proto => FIELD_PROTO.captures(l).map(|c| c[1].to_string()),
            Lang::Sql => FIELD_SQL.captures(l).map(|c| c[1].to_string()).filter(|n| {
                !matches!(
                    n.to_uppercase().as_str(),
                    "PRIMARY" | "FOREIGN" | "CONSTRAINT" | "UNIQUE" | "CHECK" | "KEY" | "INDEX"
                )
            }),
            _ => None,
        };
        if let Some(n) = name
            && !out.contains(&n)
        {
            out.push(n);
        }
    }
    out
}

/// Other code that names a schema type or a field it lost: the code that
/// fills a shader's buffer, a query's resolver, a table's queries. By name
/// only: possible.
fn users(
    code: &CodeIndex,
    r: &Ripple,
    removed: &[String],
    changed: &HashMap<(String, String), Vec<(usize, usize)>>,
) -> Vec<(String, String, usize)> {
    let mut words: Vec<String> = vec![r.name.clone()];
    for f in removed {
        words.push(f.clone());
        // created_at in SQL is createdAt in code
        let camel: String = f
            .split('_')
            .enumerate()
            .map(
                |(i, p)| if i == 0 { p.to_string() } else { p[..1.min(p.len())].to_uppercase() + &p[1.min(p.len())..] },
            )
            .collect();
        if camel != *f {
            words.push(camel);
        }
    }
    let pattern = format!(r"\b(?:{})\b", words.iter().map(|w| regex::escape(w)).collect::<Vec<_>>().join("|"));
    let Ok(re) = Regex::new(&pattern) else { return Vec::new() };
    let mut out = Vec::new();
    let Ok(files) = code.files() else { return out };
    for (folder, path, _) in files {
        if folder == r.folder && path == r.path {
            continue;
        }
        let Some(f) = code.folders.iter().find(|f| f.key == folder) else { continue };
        let Ok(text) = std::fs::read_to_string(f.dir.join(&path)) else { continue };
        if let Some((n, _)) = text.lines().enumerate().find(|(_, l)| re.is_match(l)) {
            let line = n + 1;
            let touched = changed
                .get(&(folder.clone(), path.clone()))
                .is_some_and(|rs| rs.iter().any(|(a, b)| *a <= line && line <= *b));
            if !touched {
                out.push((folder, path, line));
            }
        }
        if out.len() >= 8 {
            break;
        }
    }
    out
}

/// The callers and tests of a changed symbol.
fn fill(code: &CodeIndex, r: &mut Ripple, changed: &HashMap<(String, String), Vec<(usize, usize)>>) -> Result<()> {
    let itself = |s: &Sym| s.folder == r.folder && s.path == r.path && s.qualified == r.qualified;
    let mut texts: HashMap<(String, String), String> = HashMap::new();
    let definer_stem = r.path.rsplit('/').next().unwrap_or(&r.path).split('.').next().unwrap_or("").to_string();
    let definer_dir = r.path.rsplit_once('/').map(|p| p.0.to_string()).unwrap_or_default();
    // a Rust item that is not pub: its module and the ones under it (the
    // folder of a mod.rs, lib.rs or main.rs; the folder named as a file)
    let under = if definer_stem == "mod" || definer_stem == "lib" || definer_stem == "main" {
        definer_dir.clone()
    } else {
        r.path.rsplit_once('.').map(|p| p.0.to_string()).unwrap_or_default()
    };
    let reaches = |s: &Sym| {
        !r.private
            || (s.folder == r.folder
                && (s.path == r.path || under.is_empty() || s.path.starts_with(&format!("{under}/"))))
    };
    // a method is called by its own name: Refund, but also cart.Refund()
    for s in code.callers(&r.name)?.into_iter().filter(|s| !itself(s) && reaches(s)) {
        let updated = changed
            .get(&(s.folder.clone(), s.path.clone()))
            .is_some_and(|ranges| ranges.iter().any(|(a, b)| *a <= s.end && *b >= s.start));
        let key = (s.folder.clone(), s.path.clone());
        let text = texts.entry(key).or_insert_with(|| {
            code.folders
                .iter()
                .find(|f| f.key == s.folder)
                .and_then(|f| std::fs::read_to_string(f.dir.join(&s.path)).ok())
                .unwrap_or_default()
        });
        // the same file; in Go and Java also the same folder (one package,
        // no import)
        let package_dir = [".go", ".java", ".kt"].iter().any(|e| r.path.ends_with(e));
        let same_place = s.folder == r.folder
            && (s.path == r.path || (package_dir && s.path.rsplit_once('/').map(|p| p.0).unwrap_or("") == definer_dir));
        // called through its package or module: pricing.WithTax, tax::with_tax
        let package = definer_dir.rsplit('/').next().unwrap_or("");
        let qualified_call = [package, definer_stem.as_str()]
            .iter()
            .filter(|q| !q.is_empty())
            .any(|q| text.contains(&format!("{q}.{}", r.name)) || text.contains(&format!("{q}::{}", r.name)));
        let imports = qualified_call
            || text.lines().any(|l| {
                let l = l.trim_start();
                (l.starts_with("use ") || l.starts_with("import ") || l.starts_with("from ") || l.contains("require("))
                    && (l.contains(&r.name) || (!definer_stem.is_empty() && l.contains(&definer_stem)))
            });
        r.callers.push(Caller { sym: s, updated, sure: same_place || imports });
    }
    r.callers.sort_by_key(|c| (c.updated, !c.sure, c.sym.path.clone()));
    // tests that call it, or call what calls it
    let mut tests: Vec<Sym> = r.callers.iter().map(|c| c.sym.clone()).filter(is_test).collect();
    for c in r.callers.iter().filter(|c| !is_test(&c.sym)).take(20) {
        for t in code.callers(&c.sym.name)?.into_iter().filter(is_test) {
            tests.push(t);
        }
    }
    let mut seen = std::collections::HashSet::new();
    tests.retain(|t| seen.insert((t.folder.clone(), t.path.clone(), t.qualified.clone())));
    r.tests = tests;
    Ok(())
}

/// A test function, by the conventions of its language and file.
pub fn is_test(s: &Sym) -> bool {
    let p = s.path.to_lowercase();
    let in_test_file = p.starts_with("tests/")
        || p.contains("/tests/")
        || p.contains("/test/")
        || p.starts_with("test/")
        || p.ends_with("_test.go")
        || p.contains(".test.")
        || p.contains(".spec.")
        || p.rsplit('/').next().is_some_and(|f| f.starts_with("test_"));
    let named = s.name.starts_with("test") || s.name.starts_with("Test") || s.qualified.starts_with("tests.");
    (in_test_file || named) && matches!(s.kind.as_str(), "function" | "method")
}

/// For people and agents: what changed and where it reaches.
pub fn show(code: &CodeIndex, r: &Ripple, max_callers: usize) -> Vec<String> {
    let at = format!("{}:{}", code.display(&r.folder, &r.path), r.line);
    let mut out = vec![match &r.how {
        How::Signature { .. } => format!("{} ({at}): signature changed", r.qualified),
        How::Renamed { to } => format!("{} ({at}): renamed to {to}", r.qualified),
        How::Removed => format!("{} ({at}): removed", r.qualified),
        How::Body => format!("{} ({at})", r.qualified),
        How::Fields { .. } => format!("{} ({at}): its fields changed", r.qualified),
    }];
    if let How::Fields { removed, added } = &r.how {
        if !removed.is_empty() {
            out.push(format!("  gone or renamed: {}", removed.join(", ")));
        }
        if !added.is_empty() {
            out.push(format!("  new: {}", added.join(", ")));
        }
        if !r.users.is_empty() {
            let at: Vec<String> = r.users.iter().map(|(f, p, l)| format!("{}:{l}", code.display(f, p))).collect();
            out.push(format!("  code that names it or its old fields (possible): {}", at.join(", ")));
        }
    }
    if let How::Signature { was, now } = &r.how {
        out.push(format!("  was: {was}"));
        out.push(format!("  now: {now}"));
    }
    let name = |c: &Caller| {
        let mut s = format!("{}#{} (line {})", code.display(&c.sym.folder, &c.sym.path), c.sym.qualified, c.sym.start);
        if !c.sure {
            s.push_str(", possible");
        }
        s
    };
    let list = |v: Vec<&Caller>| {
        let more = v.len().saturating_sub(max_callers);
        let mut s = v.iter().take(max_callers).map(|c| name(c)).collect::<Vec<_>>().join(", ");
        if more > 0 {
            s.push_str(&format!(" (+{more})"));
        }
        s
    };
    let (left, done): (Vec<&Caller>, Vec<&Caller>) = r.callers.iter().partition(|c| !c.updated);
    match r.how {
        How::Fields { .. } => {}
        How::Body => {
            if !r.callers.is_empty() {
                out.push(format!("  called by: {}", list(r.callers.iter().collect())));
            }
        }
        _ => {
            if !left.is_empty() {
                out.push(format!("  not updated: {}", list(left)));
            }
            if !done.is_empty() {
                out.push(format!("  updated: {}", list(done)));
            }
            if r.callers.is_empty() {
                out.push("  nothing here calls it".into());
            }
        }
    }
    if !r.tests.is_empty() {
        let more = r.tests.len().saturating_sub(max_callers);
        let names: Vec<String> = r
            .tests
            .iter()
            .take(max_callers)
            .map(|t| format!("{}#{}", code.display(&t.folder, &t.path), t.qualified))
            .collect();
        out.push(format!(
            "  tests: {}{}",
            names.join(", "),
            if more > 0 { format!(" (+{more})") } else { String::new() }
        ));
    }
    out
}

/// One line for what a change leaves broken, for pg run, hooks and finish.
pub fn warning(code: &CodeIndex, r: &Ripple) -> Option<String> {
    if !r.breaks() {
        return None;
    }
    let what = match &r.how {
        How::Signature { .. } => "its signature changed",
        How::Renamed { .. } => "it was renamed",
        How::Removed => "it was removed",
        How::Body => return None,
        How::Fields { removed, .. } => {
            let at: Vec<String> =
                r.users.iter().take(3).map(|(f, p, l)| format!("{}:{l}", code.display(f, p))).collect();
            let lost =
                if removed.is_empty() { String::new() } else { format!(" (gone or renamed: {})", removed.join(", ")) };
            return Some(format!(
                "{}: its fields changed{lost}; code that may fill or read it: {} (pg ripple {})",
                r.qualified,
                at.join(", "),
                r.name
            ));
        }
    };
    let left = r.not_updated();
    let shown: Vec<String> = left
        .iter()
        .take(3)
        .map(|c| {
            let p = if c.sure { "" } else { ", possible" };
            format!("{}:{}{p}", code.display(&c.sym.folder, &c.sym.path), c.sym.start)
        })
        .collect();
    let more = left.len().saturating_sub(3);
    Some(format!(
        "{}: {what}; not updated: {}{} (pg ripple {})",
        r.qualified,
        shown.join(", "),
        if more > 0 { format!(" +{more}") } else { String::new() },
        r.name
    ))
}
