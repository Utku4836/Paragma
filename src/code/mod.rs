//! The code of the folders linked to a project (`.pg/code.db`): files and
//! their symbols, kept up to date like the notes index (only changed files
//! are read again, on every core), and derived only from the code, so it
//! can be deleted at any time.

pub mod files;
pub mod git;
pub mod lang;
pub mod notes;
pub mod parse;
pub mod stale;
pub mod text;
pub mod work;

use crate::embed;
use crate::project::{Home, Project};
use crate::state::links::Links;
use anyhow::Result;
use lang::Lang;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::PathBuf;

/// Bumped whenever the tables, the parsing or the files read change
/// (4: files marked linguist-vendored or -generated are left out).
const SCHEMA_VERSION: &str = "4";

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE files(
    folder TEXT,
    path TEXT,
    lang TEXT,
    mtime INTEGER,
    size INTEGER,
    PRIMARY KEY(folder, path)
);
CREATE TABLE symbols(
    rowid INTEGER PRIMARY KEY,
    folder TEXT,
    path TEXT,
    name TEXT,
    qualified TEXT,
    kind TEXT,
    start INTEGER,
    end INTEGER,
    signature TEXT,
    doc TEXT,
    calls TEXT,
    body_hash INTEGER,
    vec BLOB
);
CREATE INDEX symbols_file ON symbols(folder, path);
CREATE INDEX symbols_name ON symbols(name);
";

/// A code folder linked to the project.
#[derive(Clone, Debug)]
pub struct Folder {
    /// As .links writes it.
    pub key: String,
    pub dir: PathBuf,
    /// Its last part: "shop-api".
    pub name: String,
}

pub struct CodeIndex {
    pub db: Connection,
    pub folders: Vec<Folder>,
}

#[derive(Clone, Debug)]
pub struct Sym {
    pub folder: String,
    pub path: String,
    pub name: String,
    pub qualified: String,
    pub kind: String,
    pub start: usize,
    pub end: usize,
    pub signature: String,
    pub doc: String,
    pub calls: Vec<String>,
    pub body_hash: u64,
}

/// A symbol that moved or was renamed: the same body under another name or
/// in another file.
#[derive(Clone, Debug)]
pub struct Rename {
    pub old_path: String,
    pub old: String,
    pub new_path: String,
    pub new: String,
}

/// The commit the project's code is at (its first git folder), to keep
/// with what is written about it.
pub fn head_commit(home: &Home, project: &Project) -> Option<String> {
    Links::load(home).folders_of(&project.name).iter().find_map(|f| git::head(std::path::Path::new(f)))
}

impl CodeIndex {
    /// The project's code index, brought up to date; None when no code
    /// folder is linked to the project. Renamed symbols found on the way
    /// are returned so notes can follow them.
    pub fn open(home: &Home, project: &Project) -> Result<Option<(CodeIndex, Vec<Rename>)>> {
        let folders: Vec<Folder> = Links::load(home)
            .folders_of(&project.name)
            .into_iter()
            .map(|key| {
                let dir = PathBuf::from(&key);
                let name = key.rsplit('/').next().unwrap_or(&key).to_string();
                Folder { key, dir, name }
            })
            .filter(|f| f.dir.is_dir())
            .collect();
        if folders.is_empty() {
            return Ok(None);
        }
        let db = crate::db::open(&project.meta_dir().join("code.db"))?;
        let mut index = CodeIndex { db, folders };
        index.ensure_schema()?;
        let renames = index.sync()?;
        Ok(Some((index, renames)))
    }

    /// The project's code index as it was last brought up to date, without
    /// looking for changes: for what may lag a little (documents in search).
    /// None when there is none yet.
    pub fn open_as_is(home: &Home, project: &Project) -> Result<Option<CodeIndex>> {
        let path = project.meta_dir().join("code.db");
        if !path.exists() {
            return Ok(None);
        }
        let folders: Vec<Folder> = Links::load(home)
            .folders_of(&project.name)
            .into_iter()
            .map(|key| {
                let dir = PathBuf::from(&key);
                let name = key.rsplit('/').next().unwrap_or(&key).to_string();
                Folder { key, dir, name }
            })
            .collect();
        let db = crate::db::open(&path)?;
        let wanted = format!("{SCHEMA_VERSION}/{}", embed::fingerprint());
        let have: Option<String> = db.query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| r.get(0)).ok();
        Ok((have.as_deref() == Some(wanted.as_str())).then_some(CodeIndex { db, folders }))
    }

    fn ensure_schema(&mut self) -> Result<()> {
        let wanted = format!("{SCHEMA_VERSION}/{}", embed::fingerprint());
        let version = |db: &Connection| -> Option<String> {
            db.query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| r.get(0)).ok()
        };
        if version(&self.db).as_deref() == Some(wanted.as_str()) {
            return Ok(());
        }
        let tx = self.db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if version(&tx).as_deref() != Some(wanted.as_str()) {
            tx.execute_batch("DROP TABLE IF EXISTS meta; DROP TABLE IF EXISTS files; DROP TABLE IF EXISTS symbols;")?;
            tx.execute_batch(SCHEMA)?;
            tx.execute("INSERT INTO meta(key, value) VALUES ('schema', ?1)", [&wanted])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Reads again the files that changed since the last look.
    fn sync(&mut self) -> Result<Vec<Rename>> {
        let mut changed: Vec<Stale> = Vec::new(); // folder, path, now (None: gone)
        for f in &self.folders {
            let mut known: HashMap<String, (i64, u64)> = HashMap::new();
            let mut st = self.db.prepare("SELECT path, mtime, size FROM files WHERE folder = ?1")?;
            for r in
                st.query_map([&f.key], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?
            {
                let (p, m, s) = r?;
                known.insert(p, (m, s as u64));
            }
            crate::output::mark("code: listing");
            let listed = files::list(&f.dir);
            crate::output::mark("code: listed");
            for path in listed {
                let Ok(meta) = std::fs::metadata(f.dir.join(&path)) else { continue };
                if meta.len() > files::MAX_SIZE {
                    continue;
                }
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos() as i64)
                    .unwrap_or(0);
                if known.remove(&path) != Some((mtime, meta.len())) {
                    changed.push((f.key.clone(), path, Some((mtime, meta.len()))));
                }
            }
            changed.extend(known.into_keys().map(|p| (f.key.clone(), p, None)));
        }
        if changed.is_empty() {
            return Ok(Vec::new());
        }
        crate::output::mark("code: changed found");
        let parsed = parse_all(&self.folders, &changed);
        let tx = self.db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut removed: Vec<(String, String, String, i64)> = Vec::new(); // folder, path, qualified, hash
        for (folder, path, _) in &changed {
            let mut st =
                tx.prepare_cached("SELECT qualified, body_hash FROM symbols WHERE folder = ?1 AND path = ?2")?;
            for r in st.query_map(params![folder, path], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (q, h) = r?;
                removed.push((folder.clone(), path.clone(), q, h));
            }
            tx.execute("DELETE FROM symbols WHERE folder = ?1 AND path = ?2", params![folder, path])?;
            tx.execute("DELETE FROM files WHERE folder = ?1 AND path = ?2", params![folder, path])?;
        }
        let mut added: Vec<(String, String, String, i64)> = Vec::new();
        for ((folder, path, now), symbols) in changed.iter().zip(parsed) {
            let Some((mtime, size)) = now else { continue };
            let lang = Lang::of(path).map(Lang::name).unwrap_or("");
            tx.execute(
                "INSERT INTO files(folder, path, lang, mtime, size) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![folder, path, lang, mtime, *size as i64],
            )?;
            let mut insert = tx.prepare_cached(
                "INSERT INTO symbols(folder, path, name, qualified, kind, start, end, signature, doc, calls, body_hash, vec)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            for (s, v) in symbols {
                insert.execute(params![
                    folder,
                    path,
                    s.name,
                    s.qualified,
                    s.kind,
                    s.start as i64,
                    s.end as i64,
                    s.signature,
                    s.doc,
                    s.calls.join(" "),
                    s.body_hash as i64,
                    embed::to_bytes(&v)
                ])?;
                added.push((folder.clone(), path.clone(), s.qualified, s.body_hash as i64));
            }
        }
        tx.commit()?;
        Ok(renames(&removed, &added))
    }

    /// How a file is called in output: its path, after its folder's name
    /// when the project has several folders.
    pub fn display(&self, folder: &str, path: &str) -> String {
        match self.folders.iter().find(|f| f.key == folder) {
            Some(f) if self.folders.len() > 1 => format!("{}/{path}", f.name),
            _ => path.to_string(),
        }
    }

    /// Every file: (folder, path, language).
    pub fn files(&self) -> Result<Vec<(String, String, String)>> {
        let mut st = self.db.prepare("SELECT folder, path, lang FROM files ORDER BY folder, path")?;
        let rows =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// A file named by its path, its displayed path, or the end of its path
    /// (when only one file ends so): (folder, path).
    pub fn file(&self, given: &str) -> Result<Option<(String, String)>> {
        let given = given.trim().trim_start_matches("./").replace('\\', "/");
        let all = self.files()?;
        if let Some((f, p, _)) = all.iter().find(|(f, p, _)| *p == given || self.display(f, p) == given) {
            return Ok(Some((f.clone(), p.clone())));
        }
        let ends: Vec<_> =
            all.iter().filter(|(_, p, _)| p.ends_with(&format!("/{given}")) || p.ends_with(&given)).collect();
        Ok(match ends.as_slice() {
            [(f, p, _)] => Some((f.clone(), p.clone())),
            _ => None,
        })
    }

    /// A file named in tool output: its path may be absolute or relative to
    /// somewhere else, so the longest end of it that names one file wins.
    pub fn locate(&self, file: &str) -> Result<Option<(String, String)>> {
        let parts: Vec<&str> = file.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
        for skip in 0..parts.len() {
            if let Some(found) = self.file(&parts[skip..].join("/"))? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    /// The names of each file's top symbols (not methods, not impl blocks),
    /// for the map: (folder, path) -> names in order.
    pub fn top_names(&self) -> Result<HashMap<(String, String), Vec<String>>> {
        let mut st = self.db.prepare(
            "SELECT folder, path, name FROM symbols WHERE instr(qualified, '.') = 0 AND kind != 'impl' ORDER BY folder, path, start",
        )?;
        let mut out: HashMap<(String, String), Vec<String>> = HashMap::new();
        for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
            let (f, p, n) = r?;
            out.entry((f, p)).or_default().push(n);
        }
        Ok(out)
    }

    pub fn symbols_of(&self, folder: &str, path: &str) -> Result<Vec<Sym>> {
        self.query(&format!("{SELECT} WHERE folder = ?1 AND path = ?2 ORDER BY start"), params![folder, path])
    }

    /// Symbols called `name` (or "Type.name"), exact case first.
    pub fn named(&self, name: &str) -> Result<Vec<Sym>> {
        let rows = self.query(&format!("{SELECT} WHERE name = ?1 OR qualified = ?1"), params![name])?;
        if !rows.is_empty() {
            return Ok(rows);
        }
        self.query(&format!("{SELECT} WHERE lower(name) = lower(?1) OR lower(qualified) = lower(?1)"), params![name])
    }

    /// The innermost symbol around a line of a file.
    pub fn at(&self, folder: &str, path: &str, line: usize) -> Result<Option<Sym>> {
        Ok(self
            .query(
                &format!("{SELECT} WHERE folder = ?1 AND path = ?2 AND start <= ?3 AND end >= ?3 ORDER BY end - start LIMIT 1"),
                params![folder, path, line as i64],
            )?
            .pop())
    }

    /// Symbols that call `name`.
    pub fn callers(&self, name: &str) -> Result<Vec<Sym>> {
        let like = format!("% {name} %");
        let rows = self.query(&format!("{SELECT} WHERE ' ' || calls || ' ' LIKE ?1"), params![like])?;
        Ok(rows)
    }

    /// Symbols closest in meaning to a text, with their similarity.
    pub fn nearest(&self, text: &str, n: usize) -> Result<Vec<(Sym, f32)>> {
        self.nearest_where(text, n, "kind != 'section'")
    }

    /// Sections of the project's Markdown files closest in meaning to a text.
    pub fn nearest_docs(&self, text: &str, n: usize) -> Result<Vec<(Sym, f32)>> {
        self.nearest_where(text, n, "kind = 'section'")
    }

    fn nearest_where(&self, text: &str, n: usize, filter: &str) -> Result<Vec<(Sym, f32)>> {
        let q = embed::encoder().encode(text);
        let mut st = self.db.prepare(&format!("SELECT rowid, vec FROM symbols WHERE {filter}"))?;
        let mut scored: Vec<(i64, f32)> = st
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
            .filter_map(|r| r.ok())
            .map(|(id, v)| (id, embed::similarity(&q, &embed::from_bytes(&v))))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut out = Vec::new();
        for (id, s) in scored.into_iter().take(n) {
            if let Some(sym) = self.query(&format!("{SELECT} WHERE rowid = ?1"), params![id])?.pop() {
                out.push((sym, s));
            }
        }
        Ok(out)
    }

    pub fn counts(&self) -> (i64, i64) {
        let n = |sql: &str| self.db.query_row(sql, [], |r| r.get(0)).unwrap_or(0);
        (n("SELECT COUNT(*) FROM files"), n("SELECT COUNT(*) FROM symbols"))
    }

    fn query(&self, sql: &str, p: impl rusqlite::Params) -> Result<Vec<Sym>> {
        let mut st = self.db.prepare_cached(sql)?;
        let rows = st
            .query_map(p, |r| {
                Ok(Sym {
                    folder: r.get(0)?,
                    path: r.get(1)?,
                    name: r.get(2)?,
                    qualified: r.get(3)?,
                    kind: r.get(4)?,
                    start: r.get::<_, i64>(5)? as usize,
                    end: r.get::<_, i64>(6)? as usize,
                    signature: r.get(7)?,
                    doc: r.get(8)?,
                    calls: r.get::<_, String>(9)?.split(' ').filter(|s| !s.is_empty()).map(str::to_string).collect(),
                    body_hash: r.get::<_, i64>(10)? as u64,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

/// A file to read again: its folder, its path, and its (mtime, size) now
/// (None: it is gone).
type Stale = (String, String, Option<(i64, u64)>);

const SELECT: &str =
    "SELECT folder, path, name, qualified, kind, start, end, signature, doc, calls, body_hash FROM symbols";

/// Parses changed files on every core, each symbol with its meaning vector
/// (its name's words, its comment, its file); gone files get none.
fn parse_all(folders: &[Folder], changed: &[Stale]) -> Vec<Vec<(parse::Symbol, Vec<f32>)>> {
    let dir_of = |key: &str| folders.iter().find(|f| f.key == key).map(|f| f.dir.clone());
    let one = |(folder, path, now): &Stale| -> Vec<(parse::Symbol, Vec<f32>)> {
        let (Some(_), Some(lang), Some(dir)) = (now, Lang::of(path), dir_of(folder)) else { return Vec::new() };
        match std::fs::read_to_string(dir.join(path)) {
            Ok(src) => {
                let enc = embed::encoder();
                let file = path.replace(['/', '_', '.'], " ");
                parse::symbols(lang, &src)
                    .into_iter()
                    .map(|s| {
                        let words = crate::search::words_of(&s.qualified).join(" ");
                        let v = enc.encode(&format!(
                            "{words}. {}. {}{file}",
                            s.doc,
                            if s.about.is_empty() { String::new() } else { format!("{}. ", s.about) }
                        ));
                        (s, v)
                    })
                    .collect()
            }
            Err(_) => Vec::new(),
        }
    };
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
    if changed.len() < 8 || threads == 1 {
        return changed.iter().map(one).collect();
    }
    let size = changed.len().div_ceil(threads);
    std::thread::scope(|s| {
        let handles: Vec<_> =
            changed.chunks(size).map(|part| s.spawn(move || part.iter().map(one).collect::<Vec<_>>())).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    })
}

/// Symbols gone from one place whose body showed up under another name or
/// in another file.
fn renames(removed: &[(String, String, String, i64)], added: &[(String, String, String, i64)]) -> Vec<Rename> {
    let mut out = Vec::new();
    for (f, p, q, h) in removed {
        if added.iter().any(|(f2, p2, q2, _)| f2 == f && p2 == p && q2 == q) {
            continue; // still there
        }
        let same: Vec<_> = added.iter().filter(|(_, _, _, h2)| h2 == h).collect();
        if let [(_, p2, q2, _)] = same.as_slice() {
            out.push(Rename { old_path: p.clone(), old: q.clone(), new_path: p2.clone(), new: q2.clone() });
        }
    }
    out
}
