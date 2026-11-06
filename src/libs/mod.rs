//! The libraries a project uses, at their exact versions: their real API
//! for `pg find` and `pg read`, the name a failed build was looking for in
//! `pg run`, and what changed when one was upgraded, in the brief.
//!
//! Read from the source already on this computer (deps.rs), only when
//! asked, and kept by version in `<home>/libs/libs.db`, shared by every
//! project: a second project with the same version reads nothing.

pub mod api;
pub mod deps;

use anyhow::Result;
use api::{Item, Skip};
use deps::Dep;
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashSet;
use std::sync::LazyLock;

/// Bumped when what is read from a library changes: read again.
const SCHEMA_VERSION: &str = "1";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS libs(
    id INTEGER PRIMARY KEY,
    eco TEXT, name TEXT, version TEXT, dir TEXT,
    state TEXT, items INTEGER, at INTEGER,
    UNIQUE(eco, name, version)
);
CREATE TABLE IF NOT EXISTS items(
    rowid INTEGER PRIMARY KEY,
    lib INTEGER, path TEXT, name TEXT, kind TEXT, signature TEXT, doc TEXT,
    example TEXT, feature TEXT, deprecated TEXT, file TEXT, line INTEGER, end_line INTEGER
);
CREATE INDEX IF NOT EXISTS items_lib_name ON items(lib, name);
CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(path, name, signature, doc, tokenize = 'unicode61');
CREATE TABLE IF NOT EXISTS seen(folder TEXT, eco TEXT, name TEXT, version TEXT, PRIMARY KEY(folder, eco, name));
";

/// The libraries read so far.
pub struct Store {
    db: Connection,
}

/// A library's item found, with the library.
#[derive(Debug, Clone)]
pub struct Found {
    pub dep: Dep,
    pub item: Item,
}

impl Store {
    pub fn open(home: &crate::project::Home) -> Result<Store> {
        let db = crate::db::open(&home.root.join("libs").join("libs.db"))?;
        db.execute_batch(SCHEMA)?;
        let have: Option<String> =
            db.query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| r.get(0)).optional()?;
        if have.as_deref() != Some(SCHEMA_VERSION) {
            db.execute_batch("DELETE FROM items; DELETE FROM items_fts; DELETE FROM libs;")?;
            db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('schema', ?1)", [SCHEMA_VERSION])?;
        }
        Ok(Store { db })
    }

    /// The library read, reading it now when it was not yet; None when it
    /// cannot be (not on this computer, too large).
    pub fn ensure(&mut self, dep: &Dep) -> Result<Option<i64>> {
        let known: Option<(i64, String)> = self
            .db
            .query_row(
                "SELECT id, state FROM libs WHERE eco = ?1 AND name = ?2 AND version = ?3",
                params![dep.eco, dep.name, dep.version],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, state)) = known {
            return Ok((state == "ok").then_some(id));
        }
        let read = api::items(dep);
        let tx = self.db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // another pg may have read it meanwhile
        if let Some((id, state)) = tx
            .query_row(
                "SELECT id, state FROM libs WHERE eco = ?1 AND name = ?2 AND version = ?3",
                params![dep.eco, dep.name, dep.version],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            return Ok((state == "ok").then_some(id));
        }
        let (state, items) = match read {
            Ok(items) => ("ok", items),
            Err(Skip::NotHere) => ("not here", Vec::new()),
            Err(Skip::TooBig(_)) => ("too big", Vec::new()),
        };
        let dir = dep.dir.as_ref().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        tx.execute(
            "INSERT INTO libs(eco, name, version, dir, state, items, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![dep.eco, dep.name, dep.version, dir, state, items.len() as i64, crate::output::now()],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut ins = tx.prepare(
                "INSERT INTO items(lib, path, name, kind, signature, doc, example, feature, deprecated, file, line, end_line)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            let mut fts =
                tx.prepare("INSERT INTO items_fts(rowid, path, name, signature, doc) VALUES (?1, ?2, ?3, ?4, ?5)")?;
            for i in &items {
                ins.execute(params![
                    id,
                    i.path,
                    i.name,
                    i.kind,
                    i.signature,
                    i.doc,
                    i.example,
                    i.feature,
                    i.deprecated,
                    i.file,
                    i.line as i64,
                    i.end as i64
                ])?;
                // code names are searched by their parts too: set_language, setLanguage
                let parts = format!("{} {}", i.name, split_name(&i.name));
                fts.execute(params![tx.last_insert_rowid(), i.path, parts, i.signature, i.doc])?;
            }
        }
        tx.commit()?;
        Ok((state == "ok").then_some(id))
    }

    /// Whether a library was read already (without reading it).
    pub fn read_already(&self, dep: &Dep) -> Option<i64> {
        self.db
            .query_row(
                "SELECT id FROM libs WHERE eco = ?1 AND name = ?2 AND version = ?3 AND state = 'ok'",
                params![dep.eco, dep.name, dep.version],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    /// Items of one library matching the words, the best first: a name
    /// the words name exactly, then by BM25.
    pub fn search(&self, lib: i64, words: &[String], n: usize) -> Result<Vec<Item>> {
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let terms: Vec<String> = words.iter().map(|w| format!("\"{}\"*", w.replace('"', ""))).collect();
        let mut st = self.db.prepare_cached(&format!(
            "SELECT {COLUMNS}, bm25(items_fts, 2.0, 6.0, 1.0, 0.5) AS score FROM items_fts JOIN items i ON i.rowid = items_fts.rowid
             WHERE items_fts MATCH ?1 AND i.lib = ?2 ORDER BY score LIMIT 2000"
        ))?;
        let mut found: Vec<(Item, f64)> = Vec::new();
        for expr in [terms.join(" AND "), terms.join(" OR ")] {
            found = st
                .query_map(params![expr, lib], |r| Ok((item(r)?, r.get::<_, f64>(12)?)))?
                .collect::<rusqlite::Result<_>>()?;
            if !found.is_empty() {
                break;
            }
        }
        let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        // ahead: the words that are the parts of its path (Value get ->
        // serde_json::Value::get), its name most; then BM25; then the
        // shorter path (a re-export at the top: the path code writes)
        let rank = |it: &Item| -> i64 {
            let parts: Vec<String> =
                it.path.split("::").flat_map(|p| p.split('.')).skip(1).map(str::to_lowercase).collect();
            let hits = lower.iter().filter(|w| parts.contains(w)).count() as i64;
            let named = i64::from(lower.contains(&it.name.to_lowercase()));
            -(hits * 2 + named * 3)
        };
        found.sort_by(|a, b| {
            (rank(&a.0), a.0.path.len() / 12).cmp(&(rank(&b.0), b.0.path.len() / 12)).then(a.1.total_cmp(&b.1))
        });
        let mut seen = HashSet::new();
        Ok(found
            .into_iter()
            .map(|f| f.0)
            .filter(|i| seen.insert((i.file.clone(), i.line, i.name.clone())))
            .take(n)
            .collect())
    }

    /// Items of a library with this exact path or path ending.
    pub fn at_path(&self, lib: i64, path: &str) -> Result<Vec<Item>> {
        let tail = path.rsplit([':', '.']).next().unwrap_or(path);
        let mut st = self.db.prepare_cached(&format!("SELECT {COLUMNS} FROM items i WHERE lib = ?1 AND name = ?2"))?;
        let all: Vec<Item> = st.query_map(params![lib, tail], item)?.collect::<rusqlite::Result<_>>()?;
        let norm = |p: &str| p.replace("::", ".");
        let want = norm(path);
        let mut out: Vec<Item> =
            all.into_iter().filter(|i| norm(&i.path) == want || norm(&i.path).ends_with(&format!(".{want}"))).collect();
        out.sort_by_key(|i| i.path.len());
        Ok(out)
    }

    /// Every name of a library, for names close to a missing one.
    fn all(&self, lib: i64) -> Result<Vec<Item>> {
        let mut st = self.db.prepare_cached(&format!("SELECT {COLUMNS} FROM items i WHERE lib = ?1"))?;
        let rows = st.query_map([lib], item)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    fn count(&self, lib: i64) -> i64 {
        self.db.query_row("SELECT items FROM libs WHERE id = ?1", [lib], |r| r.get(0)).unwrap_or(0)
    }
}

const COLUMNS: &str =
    "i.path, i.name, i.kind, i.signature, i.doc, i.example, i.feature, i.deprecated, i.file, i.line, i.end_line, i.lib";

fn item(r: &rusqlite::Row) -> rusqlite::Result<Item> {
    Ok(Item {
        path: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        signature: r.get(3)?,
        doc: r.get(4)?,
        example: r.get(5)?,
        feature: r.get(6)?,
        deprecated: r.get(7)?,
        file: r.get(8)?,
        line: r.get::<_, i64>(9)? as usize,
        end: r.get::<_, i64>(10)? as usize,
    })
}

/// setLanguage, set_language -> "set language".
fn split_name(name: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if c == '_' {
            out.push(' ');
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        out.push(c);
    }
    out.trim().to_string()
}

/// The libraries of a project's code folders.
pub fn of_folders(dirs: &[std::path::PathBuf]) -> Vec<Dep> {
    let mut out: Vec<Dep> = Vec::new();
    for d in dirs {
        for dep in deps::of(d) {
            if !out.iter().any(|o| o.eco == dep.eco && o.name == dep.name && o.version == dep.version) {
                out.push(dep);
            }
        }
    }
    out
}

/// What a library's item says, for an agent: where it is from, the
/// signature, the doc, the example, the feature it needs, deprecation, its
/// source. `short`: one line.
pub fn show(f: &Found, short: bool) -> String {
    let (d, i) = (&f.dep, &f.item);
    let head = format!("[lib {} {}] {}", d.name, d.version, i.path);
    if short {
        let mut s = format!("{head}: {}", i.signature);
        if !i.deprecated.is_empty() {
            s.push_str(" (deprecated)");
        }
        return s;
    }
    let mut out = vec![head, i.signature.clone()];
    if !i.doc.is_empty() {
        out.push(i.doc.clone());
    }
    if !i.deprecated.is_empty() {
        out.push(format!("deprecated: {}", i.deprecated));
    }
    if !i.feature.is_empty() {
        let on = d.features_on().map(|f| f.contains(&i.feature));
        out.push(match on {
            Some(true) => format!("needs feature \"{}\" (on)", i.feature),
            Some(false) => format!(
                "needs feature \"{}\": not on; add it: {} = {{ version = \"...\", features = [\"{}\"] }}",
                i.feature, d.name, i.feature
            ),
            None => format!("needs feature \"{}\"", i.feature),
        });
    }
    if !i.example.is_empty() {
        out.push("example:".into());
        out.extend(i.example.lines().map(|l| format!("  {l}")));
    }
    if let Some(dir) = d.dir.as_ref().filter(|_| !i.file.is_empty()) {
        let base = if dir.is_file() { dir.parent().unwrap_or(dir) } else { dir };
        out.push(format!(
            "source: {}:{}",
            base.join(i.file.replace('/', std::path::MAIN_SEPARATOR_STR)).display(),
            i.line
        ));
    }
    out.join("\n")
}

/// The words of a query that name a library, and the rest.
fn split_query<'a>(deps: &'a [Dep], query: &str) -> (Vec<&'a Dep>, Vec<String>) {
    let mut named: Vec<&Dep> = Vec::new();
    let mut rest = Vec::new();
    for token in query.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')') {
        if token.is_empty() {
            continue;
        }
        // tree_sitter::Parser, lodash.debounce: the first part may be the library
        let parts: Vec<&str> = token.split("::").flat_map(|p| p.split('.')).filter(|p| !p.is_empty()).collect();
        let mut i = 0;
        if let Some(d) = deps.iter().find(|d| d.named_by(token))
            && !named.contains(&d)
        {
            named.push(d);
            continue;
        }
        if parts.len() > 1
            && let Some(d) = deps.iter().find(|d| d.named_by(parts[0]))
        {
            if !named.contains(&d) {
                named.push(d);
            }
            i = 1;
        }
        rest.extend(parts[i..].iter().map(|p| p.to_string()));
    }
    // code names, not prose: get, set, new count
    const NOISE: &[&str] = &["the", "a", "an", "of", "in", "to", "for", "and", "or", "with", "how", "what"];
    let mut words: Vec<String> = Vec::new();
    for w in rest.iter().flat_map(|r| r.split(|c: char| !c.is_alphanumeric() && c != '_')) {
        let w = w.to_lowercase();
        if w.chars().count() >= 2 && !NOISE.contains(&w.as_str()) && !words.contains(&w) {
            words.push(w);
        }
    }
    (named, words)
}

/// For `pg find`: the API of the libraries the query names (read now if
/// needed), else exact names in those read already.
pub fn find(home: &crate::project::Home, dirs: &[std::path::PathBuf], query: &str, n: usize) -> Result<Vec<Found>> {
    // most searches name no library and no code name: decided from the
    // manifests alone, before the lock files and the library folders
    let codey = query
        .split_whitespace()
        .any(|w| w.contains('_') || w.contains("::") || w.chars().skip(1).any(char::is_uppercase));
    if !codey {
        let names: Vec<String> =
            dirs.iter().flat_map(|d| deps::names(d)).map(|n| n.to_lowercase().replace('-', "_")).collect();
        let named = query.split(|c: char| c.is_whitespace() || c == '.' || c == ',').any(|w| {
            let w = w.to_lowercase().replace('-', "_");
            !w.is_empty() && names.iter().any(|n| *n == w || n.rsplit('/').next() == Some(w.as_str()))
        });
        if !named {
            return Ok(Vec::new());
        }
    }
    let deps = of_folders(dirs);
    if deps.is_empty() {
        return Ok(Vec::new());
    }
    let (named, words) = split_query(&deps, query);
    let mut out = Vec::new();
    let raw: Vec<String> = query
        .split_whitespace()
        .filter(|w| w.contains('_') || w.chars().skip(1).any(char::is_uppercase))
        .map(str::to_string)
        .collect();
    if named.is_empty() && raw.is_empty() {
        return Ok(out);
    }
    let mut store = Store::open(home)?;
    if !named.is_empty() {
        for d in named {
            let Some(lib) = store.ensure(d)? else { continue };
            let items = if words.is_empty() { Vec::new() } else { store.search(lib, &words, n)? };
            if items.is_empty() && words.is_empty() {
                // only the library named: what it is and how to ask
                out.push(Found {
                    dep: d.clone(),
                    item: Item {
                        path: d.code_name(),
                        signature: format!(
                            "{} public items; ask for one: pg find \"{} <name>\"",
                            store.count(lib),
                            d.code_name()
                        ),
                        ..Item::default()
                    },
                });
            }
            out.extend(items.into_iter().map(|item| Found { dep: d.clone(), item }));
        }
        return Ok(out);
    }
    // no library named: a code name of the query, in the libraries read
    for d in deps.iter().filter(|d| d.direct) {
        let Some(lib) = store.read_already(d) else { continue };
        for name in &raw {
            for item in store.at_path(lib, name)?.into_iter().take(2) {
                out.push(Found { dep: d.clone(), item });
            }
        }
    }
    out.truncate(n);
    Ok(out)
}

/// For `pg read`: a library's item by its path (tree_sitter::Parser::new),
/// with its source.
pub fn read(
    home: &crate::project::Home,
    dirs: &[std::path::PathBuf],
    target: &str,
    budget: usize,
) -> Result<Option<String>> {
    let first = target.split("::").next().unwrap_or(target).split('.').next().unwrap_or(target);
    let deps = of_folders(dirs);
    let Some(dep) = deps.iter().find(|d| d.named_by(first)) else { return Ok(None) };
    let mut store = Store::open(home)?;
    let Some(lib) = store.ensure(dep)? else { return Ok(None) };
    let path = if target.contains("::") || target.contains('.') { target.to_string() } else { dep.code_name() };
    let Some(item) = store.at_path(lib, &path)?.into_iter().next() else { return Ok(None) };
    let f = Found { dep: dep.clone(), item };
    let mut b = crate::output::Budget::new(budget);
    b.add(&show(&f, false));
    if let Some(dir) = &dep.dir {
        let base = if dir.is_file() { dir.parent().unwrap_or(dir) } else { dir.as_path() };
        if let Ok(src) = std::fs::read_to_string(base.join(&f.item.file)) {
            let lines: Vec<&str> = src.lines().collect();
            let (a, z) = (f.item.line.saturating_sub(1), f.item.end.max(f.item.line).min(lines.len()));
            b.add("");
            for (k, l) in lines.iter().enumerate().take(z).skip(a) {
                if !b.add(&format!("{:>5}  {l}", k + 1)) {
                    break;
                }
            }
        }
    }
    Ok(Some(b.out))
}

static RUST_MISSING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"cannot find (?:function|value|type|struct|macro|trait|module|enum|constant|attribute macro|derive macro) `(\w+)` in (?:crate|module) `([\w:]+)`").expect("regex")
});
static RUST_NO_ITEM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"no (?:function or associated item|method|associated item|associated function|variant or associated item) named `(\w+)` found for (?:\w+ )?`?&?(?:mut )?([\w:]+)").expect("regex")
});
static RUST_IMPORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"unresolved import `([\w:]+)`").expect("regex"));
static TS_MEMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"Module '"([^"]+)"' has no exported member '(\w+)'"#).expect("regex"));
static TS_PROP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Property '(\w+)' does not exist on type '([\w.<>]+)'").expect("regex"));
static GO_UNDEFINED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"undefined: (\w+)\.(\w+)").expect("regex"));
static GO_NO_FIELD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"has no field or method (\w+)").expect("regex"));
static PY_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"module '([\w.]+)' has no attribute '(\w+)'").expect("regex"));
static PY_IMPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"cannot import name '(\w+)' from '([\w.]+)'").expect("regex"));

/// A name a build or test looked for in a library and did not find: the
/// library (when the error says) and the name, with the type it was on.
fn missing(output: &str) -> Vec<(Option<String>, String, Option<String>)> {
    let mut out: Vec<(Option<String>, String, Option<String>)> = Vec::new();
    let mut add = |lib: Option<&str>, name: &str, on: Option<&str>| {
        let v = (lib.map(str::to_string), name.to_string(), on.map(str::to_string));
        if !out.contains(&v) {
            out.push(v);
        }
    };
    for c in RUST_MISSING.captures_iter(output) {
        add(Some(c[2].split("::").next().unwrap_or(&c[2])), &c[1], None);
    }
    for c in RUST_NO_ITEM.captures_iter(output) {
        let ty = c[2].rsplit("::").next().unwrap_or(&c[2]);
        add(None, &c[1], Some(ty));
    }
    for c in RUST_IMPORT.captures_iter(output) {
        let mut parts: Vec<&str> = c[1].split("::").collect();
        if parts.len() >= 2 {
            let name = parts.pop().unwrap_or("");
            add(Some(parts[0]), name, None);
        }
    }
    for c in TS_MEMBER.captures_iter(output) {
        add(Some(&c[1]), &c[2], None);
    }
    for c in TS_PROP.captures_iter(output) {
        add(None, &c[1], Some(&c[2]));
    }
    for c in GO_UNDEFINED.captures_iter(output) {
        add(Some(&c[1]), &c[2], None);
    }
    for c in GO_NO_FIELD.captures_iter(output) {
        add(None, &c[1], None);
    }
    for c in PY_ATTR.captures_iter(output) {
        add(Some(c[1].split('.').next().unwrap_or(&c[1])), &c[2], None);
    }
    for c in PY_IMPORT.captures_iter(output) {
        add(Some(c[2].split('.').next().unwrap_or(&c[2])), &c[1], None);
    }
    out.truncate(4);
    out
}

/// How close a name is to the one looked for (lower is closer); None: not close.
fn closeness(want: &str, have: &str) -> Option<usize> {
    let (w, h) = (want.to_lowercase(), have.to_lowercase());
    if w == h {
        return Some(0); // language / LANGUAGE
    }
    let (wn, hn) = (w.replace('_', ""), h.replace('_', ""));
    if wn == hn {
        return Some(1); // setLanguage / set_language
    }
    let d = distance(&wn, &hn);
    if d <= 2 || (wn.len() > 5 && d <= 3) {
        return Some(2 + d);
    }
    if wn.len() >= 4 && (hn.contains(&wn) || wn.contains(&hn) && hn.len() >= 4) {
        return Some(6);
    }
    None
}

fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// For `pg run`: for each name a failed build looked for in a library,
/// what the library has at this version instead. Reads the libraries the
/// errors name, or (a method on a type) those the project names.
pub fn hints(home: &crate::project::Home, dirs: &[std::path::PathBuf], output: &str) -> Result<Vec<String>> {
    let wanted = missing(output);
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let deps = of_folders(dirs);
    if deps.is_empty() {
        return Ok(Vec::new());
    }
    let mut store = Store::open(home)?;
    let mut out = Vec::new();
    for (lib, name, on) in wanted {
        let looked: Vec<&Dep> = match &lib {
            Some(l) => deps.iter().filter(|d| d.named_by(l)).collect(),
            None => deps.iter().filter(|d| d.direct).collect(),
        };
        let mut best: Vec<(usize, Found)> = Vec::new();
        for d in looked {
            let Some(id) = store.ensure(d)? else { continue };
            for item in store.all(id)? {
                if let Some(ty) = &on
                    && !item.path.contains(&format!("{ty}::"))
                    && !item.path.contains(&format!("{ty}."))
                {
                    continue;
                }
                if let Some(c) = closeness(&name, &item.name) {
                    best.push((c, Found { dep: d.clone(), item }));
                }
            }
        }
        if best.is_empty() {
            continue;
        }
        best.sort_by_key(|(c, f)| (*c, f.item.path.len()));
        let f = &best[0].1;
        let what = match (&lib, &on) {
            (_, Some(ty)) => format!("{ty} has no `{name}`"),
            (Some(_), None) => format!("{} {} has no `{name}`", f.dep.name, f.dep.version),
            (None, None) => format!("no `{name}`"),
        };
        let mut line = format!("{what}; it has {}: {}", f.item.path, f.item.signature);
        if !f.item.deprecated.is_empty() {
            line.push_str(&format!(" (deprecated: {})", f.item.deprecated));
        }
        if !f.item.feature.is_empty() && f.dep.features_on().is_some_and(|on| !on.contains(&f.item.feature)) {
            line.push_str(&format!(" (needs feature \"{}\", not on)", f.item.feature));
        }
        out.push(line);
        for (_, other) in best.iter().skip(1).take(2).filter(|(c, _)| *c <= 3) {
            out.push(format!("  or {}: {}", other.item.path, other.item.signature));
        }
    }
    Ok(out)
}

/// For the brief: the libraries upgraded since the project's last brief,
/// with what changed in the names its code calls (when both versions are
/// on this computer). The versions seen are kept for next time.
pub fn upgrades(
    home: &crate::project::Home,
    dirs: &[std::path::PathBuf],
    called: &HashSet<String>,
) -> Result<Vec<String>> {
    let mut store = Store::open(home)?;
    let mut out = Vec::new();
    for dir in dirs {
        let folder = dir.to_string_lossy().to_string();
        let first_time: bool =
            store.db.query_row("SELECT COUNT(*) = 0 FROM seen WHERE folder = ?1", [&folder], |r| r.get(0))?;
        for dep in deps::of(dir).into_iter().filter(|d| d.direct) {
            let before: Option<String> = store
                .db
                .query_row(
                    "SELECT version FROM seen WHERE folder = ?1 AND eco = ?2 AND name = ?3",
                    params![folder, dep.eco, dep.name],
                    |r| r.get(0),
                )
                .optional()?;
            if before.as_deref() != Some(dep.version.as_str()) {
                store.db.execute(
                    "INSERT OR REPLACE INTO seen(folder, eco, name, version) VALUES (?1, ?2, ?3, ?4)",
                    params![folder, dep.eco, dep.name, dep.version],
                )?;
            }
            let Some(old) = before.filter(|v| *v != dep.version) else { continue };
            if first_time {
                continue;
            }
            let mut line = format!("upgraded: {} {old} -> {}", dep.name, dep.version);
            let old_dep = Dep { version: old.clone(), dir: old_dir(&dep, &old), ..dep.clone() };
            if old_dep.dir.is_some()
                && let (Some(a), Some(b)) = (store.ensure(&old_dep)?, store.ensure(&dep)?)
            {
                let changed = diff(&store.all(a)?, &store.all(b)?, called);
                if !changed.is_empty() {
                    line.push_str(&format!("; changed in what this code calls: {}", changed.join(", ")));
                }
            }
            out.push(line);
        }
    }
    Ok(out)
}

/// An older version's source, when it is still on this computer (cargo and
/// Go keep them).
fn old_dir(dep: &Dep, old: &str) -> Option<std::path::PathBuf> {
    let dir = dep.dir.as_ref()?;
    let name = dir.file_name()?.to_string_lossy().into_owned();
    let swapped = name.replace(&dep.version, old);
    (swapped != name).then(|| dir.with_file_name(swapped)).filter(|d| d.exists())
}

/// What changed between two versions in the names `called`: gone, or
/// another signature.
fn diff(old: &[Item], new: &[Item], called: &HashSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for o in old.iter().filter(|o| called.contains(&o.name) && o.kind != "re-export") {
        let now: Vec<&Item> = new.iter().filter(|n| n.path == o.path).collect();
        if now.is_empty() {
            let elsewhere = new.iter().find(|n| n.name == o.name && n.kind == o.kind);
            out.push(match elsewhere {
                Some(n) => format!("{} moved to {}", o.path, n.path),
                None => format!("{} is gone", o.path),
            });
        } else if !now.iter().any(|n| n.signature == o.signature) {
            out.push(format!("{} is now `{}`", o.path, now[0].signature));
        } else if let Some(n) = now.iter().find(|n| !n.deprecated.is_empty())
            && o.deprecated.is_empty()
        {
            out.push(format!("{} is deprecated: {}", o.path, n.deprecated));
        }
        if out.len() >= 6 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_failed_build_looked_for() {
        let rust = "error[E0425]: cannot find function `language` in crate `tree_sitter_rust`\n --> src/lib.rs:4:20\nerror[E0599]: no method named `set_lang` found for struct `Parser` in the current scope\nerror[E0432]: unresolved import `serde_json::Valu`";
        let m = missing(rust);
        assert!(m.contains(&(Some("tree_sitter_rust".into()), "language".into(), None)), "{m:?}");
        assert!(m.contains(&(None, "set_lang".into(), Some("Parser".into()))), "{m:?}");
        assert!(m.contains(&(Some("serde_json".into()), "Valu".into(), None)), "{m:?}");
        let ts = "src/a.ts(3,10): error TS2305: Module '\"lodash\"' has no exported member 'debounced'.";
        assert_eq!(missing(ts), vec![(Some("lodash".into()), "debounced".into(), None)]);
        let go = "./main.go:9:6: undefined: cobra.Comand";
        assert_eq!(missing(go), vec![(Some("cobra".into()), "Comand".into(), None)]);
        let py = "AttributeError: module 'requests' has no attribute 'gett'";
        assert_eq!(missing(py), vec![(Some("requests".into()), "gett".into(), None)]);
    }

    #[test]
    fn close_names() {
        assert_eq!(closeness("language", "LANGUAGE"), Some(0));
        assert_eq!(closeness("setLanguage", "set_language"), Some(1));
        assert!(closeness("Comand", "Command").is_some());
        assert!(closeness("get", "post").is_none());
        assert_eq!(split_name("setLanguage"), "set Language");
        assert_eq!(split_name("set_language"), "set language");
    }
}
