//! The conversations of each agent: title, folder and last use, read from
//! where the agent keeps them. Only reads (SQLite files are opened read-only)
//! and skips what it cannot parse: an agent changing its format loses its
//! list, never breaks setup.

use rusqlite::{Connection, OpenFlags};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub struct Conversation {
    pub title: String,
    pub folder: PathBuf,
    /// Unix seconds.
    pub updated: i64,
}

/// The conversations of an agent whose folders still exist (and are not
/// temporary or app-data folders), newest first.
pub fn of(agent: &str) -> Vec<Conversation> {
    let home = std::env::home_dir().unwrap_or_default();
    let mut all = match agent {
        "claude" => claude(&home.join(".claude").join("projects")),
        "codex" => codex(&home.join(".codex")),
        "antigravity" => antigravity(&home.join(".gemini")),
        "opencode" => opencode(&home.join(".local").join("share").join("opencode")),
        _ => Vec::new(),
    };
    let hidden: Vec<String> = [Some(std::env::temp_dir()), env_dir("LOCALAPPDATA"), env_dir("APPDATA")]
        .into_iter()
        .flatten()
        .map(|p| key(&p))
        .collect();
    all.retain(|c| {
        let k = key(&c.folder);
        c.folder.is_dir() && !hidden.iter().any(|h| k.starts_with(h.as_str()))
    });
    for c in &mut all {
        c.title = c.title.split_whitespace().collect::<Vec<_>>().join(" ");
        if c.title.is_empty() {
            c.title = "(untitled)".into();
        }
    }
    all.sort_by_key(|c| std::cmp::Reverse(c.updated));
    // an agent with several apps may list one conversation in each
    all.dedup_by(|a, b| a.updated == b.updated && a.title == b.title && a.folder == b.folder);
    all
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// A folder for comparing: '/', lower case on Windows, ending in '/'.
fn key(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/').to_string() + "/";
    if cfg!(windows) { s.to_lowercase() } else { s }
}

fn folder(raw: &str) -> PathBuf {
    let s = raw.strip_prefix(r"\\?\").unwrap_or(raw);
    PathBuf::from(if cfg!(windows) { s.replace('/', "\\") } else { s.to_string() })
}

fn read_only(path: &Path) -> Option<Connection> {
    if !path.is_file() {
        return None;
    }
    let db =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    db.busy_timeout(std::time::Duration::from_secs(2)).ok()?;
    Some(db)
}

/// The first non-empty line of a message, without markdown marks.
fn first_line(s: &str) -> String {
    s.lines()
        .map(|l| l.trim().trim_start_matches(['#', '>', '*', '-']).trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

// Claude Code: ~/.claude/projects/<folder>/<session>.jsonl. The folder is in
// the first lines; the title is the last one the app set (custom-title),
// else the first thing the user typed. Session files grow to hundreds of
// MB, so only their start and end are read.

const HEAD: u64 = 512 * 1024;
const TAIL: u64 = 2 * 1024 * 1024;

fn claude(projects: &Path) -> Vec<Conversation> {
    let mut out = Vec::new();
    let Ok(dirs) = std::fs::read_dir(projects) else { return out };
    for dir in dirs.flatten() {
        let Ok(files) = std::fs::read_dir(dir.path()) else { continue };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let updated = f
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            if let Some((folder, title)) = claude_session(&p) {
                out.push(Conversation { title, folder, updated });
            }
        }
    }
    out
}

fn claude_session(p: &Path) -> Option<(PathBuf, String)> {
    let mut f = std::fs::File::open(p).ok()?;
    let len = f.metadata().ok()?.len();
    let mut head = Vec::new();
    (&mut f).take(HEAD).read_to_end(&mut head).ok()?;
    let mut tail = Vec::new();
    if len > HEAD {
        f.seek(SeekFrom::Start(len.saturating_sub(TAIL).max(HEAD))).ok()?;
        f.take(TAIL).read_to_end(&mut tail).ok()?;
    }
    let lines = |b: &[u8]| -> Vec<serde_json::Value> {
        String::from_utf8_lossy(b).lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    };
    let (head, tail) = (lines(&head), lines(&tail));
    // a continued session starts with a long summary: its folder may only be near the end
    let dir = head.iter().chain(&tail).find_map(|e| e["cwd"].as_str().filter(|s| !s.is_empty()).map(folder))?;
    let first = head.iter().filter(|e| e["type"] == "user" && e["isMeta"] != true).find_map(|e| {
        let content = &e["message"]["content"];
        let text = match content.as_str() {
            Some(s) => Some(s),
            None => content.as_array()?.iter().find_map(|part| part["text"].as_str()),
        }?;
        let typed =
            !text.trim().is_empty() && !text.starts_with('<') && !text.starts_with("This session is being continued");
        typed.then(|| first_line(text))
    });
    let title = head
        .iter()
        .chain(&tail)
        .filter(|e| e["type"] == "custom-title")
        .filter_map(|e| e["customTitle"].as_str())
        .rfind(|s| !s.is_empty())
        .map(str::to_string)
        .or(first)?; // nothing typed: an empty session
    Some((dir, title))
}

// Codex: ~/.codex/state_N.sqlite (the newest N), table threads.

fn codex(data: &Path) -> Vec<Conversation> {
    let newest = std::fs::read_dir(data)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("state_") && n.ends_with(".sqlite"))
        })
        .max_by_key(|p| {
            let n = p.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            n.trim_start_matches("state_").parse::<u32>().unwrap_or(0)
        });
    let Some(db) = newest.as_deref().and_then(read_only) else { return Vec::new() };
    // a name the user gave, in newer versions
    let queries = [
        "SELECT COALESCE(NULLIF(name, ''), title), cwd, updated_at FROM threads WHERE archived = 0",
        "SELECT title, cwd, updated_at FROM threads WHERE archived = 0",
    ];
    for sql in queries {
        let rows = db.prepare(sql).and_then(|mut s| {
            s.query_map([], |r| {
                Ok(Conversation {
                    title: codex_title(&r.get::<_, Option<String>>(0)?.unwrap_or_default()),
                    folder: folder(&r.get::<_, String>(1)?),
                    updated: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
        });
        if let Ok(rows) = rows {
            return rows;
        }
    }
    Vec::new()
}

/// A Codex title starts with the files attached, when there are any; the
/// request comes after "## My request:".
fn codex_title(raw: &str) -> String {
    first_line(raw.split_once("## My request:").map_or(raw, |(_, request)| request))
}

// Antigravity: ~/.gemini/antigravity{,-cli}/conversation_summaries.db.

fn antigravity(data: &Path) -> Vec<Conversation> {
    let mut out = Vec::new();
    for app in ["antigravity-cli", "antigravity"] {
        let Some(db) = read_only(&data.join(app).join("conversation_summaries.db")) else { continue };
        let rows = db
            .prepare("SELECT title, workspace_uris, CAST(strftime('%s', last_modified_time) AS INTEGER) FROM conversation_summaries")
            .and_then(|mut s| {
                s.query_map([], |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                        r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()
            })
            .unwrap_or_default();
        for (title, uris, updated) in rows {
            let first: Option<String> =
                serde_json::from_str::<Vec<String>>(&uris).ok().and_then(|v| v.into_iter().next());
            if let Some(dir) = first.as_deref().and_then(from_file_uri) {
                out.push(Conversation { title, folder: dir, updated });
            }
        }
    }
    out
}

/// file:///c%3A/Users/x -> C:\Users\x
fn from_file_uri(u: &str) -> Option<PathBuf> {
    let path = u.strip_prefix("file://")?;
    let mut bytes = Vec::new();
    let mut it = path.bytes();
    while let Some(b) = it.next() {
        if b == b'%' {
            let hex: String = it.by_ref().take(2).map(char::from).collect();
            bytes.push(u8::from_str_radix(&hex, 16).ok()?);
        } else {
            bytes.push(b);
        }
    }
    let s = String::from_utf8(bytes).ok()?;
    let s = match s.as_bytes() {
        [b'/', _, b':', ..] => &s[1..], // /c:/Users -> c:/Users
        _ => &s,
    };
    Some(folder(s))
}

// OpenCode: ~/.local/share/opencode/opencode.db, table session_v2 in newer
// versions, session in older ones.

fn opencode(data: &Path) -> Vec<Conversation> {
    let Some(db) = read_only(&data.join("opencode.db")) else { return Vec::new() };
    for table in ["session_v2", "session"] {
        let sql = format!("SELECT COALESCE(title, ''), directory, time_updated FROM {table} WHERE parent_id IS NULL");
        let rows = db.prepare(&sql).and_then(|mut s| {
            s.query_map([], |r| {
                Ok(Conversation {
                    title: r.get(0)?,
                    folder: folder(&r.get::<_, String>(1)?),
                    updated: r.get::<_, i64>(2)? / 1000,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
        });
        if let Ok(rows) = rows
            && (!rows.is_empty() || table == "session")
        {
            return rows;
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_become_folders() {
        let p = from_file_uri("file:///c%3A/Users/me/My%20App").unwrap();
        if cfg!(windows) {
            assert_eq!(p, PathBuf::from(r"c:\Users\me\My App"));
        } else {
            assert_eq!(p, PathBuf::from("c:/Users/me/My App"));
        }
        assert_eq!(from_file_uri("file:///home/me/app").unwrap(), PathBuf::from("/home/me/app"));
        assert!(from_file_uri("https://x").is_none());
    }

    #[test]
    fn a_claude_session_gives_its_folder_and_last_title() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.jsonl");
        let lines = [
            r#"{"type":"system","cwd":"/work/app"}"#,
            r#"{"type":"user","isMeta":true,"message":{"content":"<command>"}}"#,
            r#"{"type":"user","message":{"content":"> Fix the login\nmore"}}"#,
            r#"{"type":"custom-title","customTitle":"Login"}"#,
            r#"{"type":"custom-title","customTitle":"Login fix"}"#,
        ];
        std::fs::write(&p, lines.join("\n")).unwrap();
        let (dir, title) = claude_session(&p).unwrap();
        assert_eq!(dir, folder("/work/app"));
        assert_eq!(title, "Login fix");
        std::fs::write(&p, lines[..3].join("\n")).unwrap();
        assert_eq!(claude_session(&p).unwrap().1, "Fix the login");
        std::fs::write(&p, lines[..2].join("\n")).unwrap();
        assert!(claude_session(&p).is_none(), "nothing typed: an empty session");
    }

    #[test]
    fn a_codex_title_is_the_request_after_attachments() {
        assert_eq!(codex_title("Fix the build\nmore"), "Fix the build");
        let raw = "# Files mentioned by the user:\n\n## a.png: C:/a.png\nImage attachment: true\n\n## My request:\nmake it blue";
        assert_eq!(codex_title(raw), "make it blue");
    }
}
