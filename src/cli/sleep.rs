//! The sleep path: heavy work done after `pg finish`, in a short background
//! pg, so no command waits for it. It looks over the whole memory (which
//! notes may be outdated, what could be tidier) and keeps what it found in
//! state.db; the brief, `pg finish` and the kernel only read that.
//!
//! `PARAGMA_SLEEP=now` does it at once, in the same process (tests);
//! `PARAGMA_SLEEP=off` not at all.

use super::Context;
use crate::index::Index;
use crate::kernel::tidy::{self, NoteFacts};
use crate::note::Kind;
use crate::output::now;
use crate::project::Project;
use crate::state::State;
use anyhow::Result;
use serde_json::json;

#[derive(clap::Args)]
pub struct Args {}

/// What the last sleep found.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub at: i64,
    /// (id, why) of the notes that may be outdated.
    pub outdated: Vec<(String, String)>,
    /// What could be tidier, each with its command.
    pub tidy: Vec<String>,
}

const KEY: &str = "sleep";

pub fn run(cx: &Context, _a: Args) -> Result<()> {
    let project = cx.need_project()?;
    // one at a time a project: another finish while this one works waits for the next
    let Some(_lock) = SleepLock::try_take(project)? else { return Ok(()) };
    let found = look(cx, project)?;
    State::open(project)?.set(
        KEY,
        &json!({
            "at": found.at,
            "outdated": found.outdated.iter().map(|(i, w)| json!([i, w])).collect::<Vec<_>>(),
            "tidy": found.tidy,
        })
        .to_string(),
    )
}

/// Looks over the whole memory now.
pub fn look(cx: &Context, project: &Project) -> Result<Found> {
    let index = Index::open(project)?;
    Ok(Found { at: now(), outdated: outdated(cx, project, &index)?, tidy: tidiness(project, &index)? })
}

/// What the last sleep found (none yet: empty).
pub fn found(state: &State) -> Found {
    let Some(v) = state.get(KEY).and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) else {
        return Found::default();
    };
    Found {
        at: v["at"].as_i64().unwrap_or(0),
        outdated: v["outdated"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some((p[0].as_str()?.to_string(), p[1].as_str()?.to_string())))
            .collect(),
        tidy: v["tidy"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(str::to_string)).collect(),
    }
}

/// Starts the sleep after a finish: in the background, or as the
/// environment says.
pub fn start(cx: &Context, project: &Project) {
    match std::env::var("PARAGMA_SLEEP").as_deref() {
        Ok("off") => {}
        Ok("now") => {
            let _ = run(cx, Args {});
        }
        _ => {
            let Ok(exe) = std::env::current_exe() else { return };
            let mut c = std::process::Command::new(exe);
            c.args(["-p", &project.name, "sleep"])
                .current_dir(&cx.cwd)
                .env("PARAGMA_HOME", &cx.home.root)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            let _ = crate::sync::detach(&mut c);
        }
    }
}

/// Every note that names its code and may be outdated, with why.
fn outdated(cx: &Context, project: &Project, index: &Index) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut stale = crate::code::stale::Staleness::new(&cx.home, project);
    for row in index.notes()? {
        let props = index.props_at(&row.path);
        let names_code = props["code_state"].is_string()
            || (props["commit"].is_string() && (props["files"].is_array() || props["symbols"].is_array()));
        if !names_code || props["status"].as_str() == Some("superseded") {
            continue;
        }
        if let Some(d) = stale.drift(&props)?.filter(|d| !d.is_empty()) {
            let id = if row.id.is_empty() { row.path.trim_end_matches(".md").to_string() } else { row.id.clone() };
            out.push((id, d.text()));
        }
    }
    Ok(out)
}

/// A note as the tidying expert sees it: each `##` section with its
/// meaning vector (a short note is one piece in the index, but its
/// sections may still be about different things).
fn facts_of(project: &Project, id: &str, rel: &str) -> Option<NoteFacts> {
    let text = crate::project::files::read_text(&project.dir.join(rel)).ok()?;
    let note = crate::note::Note::parse(&text);
    let enc = crate::embed::encoder();
    let mut pieces: Vec<(String, Vec<f32>)> = Vec::new();
    let mut heading = String::new();
    let mut body = String::new();
    let flush = |heading: &str, body: &str, pieces: &mut Vec<(String, Vec<f32>)>| {
        if !body.trim().is_empty() {
            pieces.push((heading.to_string(), enc.encode(&format!("{heading}. {body}"))));
        }
    };
    for line in note.body.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            flush(&heading, &body, &mut pieces);
            heading = h.trim().to_string();
            body.clear();
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    flush(&heading, &body, &mut pieces);
    let path = rel.trim_end_matches(".md").to_string();
    Some(NoteFacts { id: if id.is_empty() { path.clone() } else { id.to_string() }, path, pieces })
}

/// Whether the note at `rel` holds several unrelated topics: the line that
/// says so, at once when it is written.
pub fn topics_of(project: &Project, rel: &str) -> Option<String> {
    let rel = if rel.ends_with(".md") { rel.to_string() } else { format!("{rel}.md") };
    let id = crate::note::Note::parse(&crate::project::files::read_text(&project.dir.join(&rel)).ok()?)
        .id()
        .unwrap_or("")
        .to_string();
    let note = facts_of(project, &id, &rel)?;
    tidy::tidy(std::slice::from_ref(&note))
        .into_iter()
        .find(|t| matches!(t, tidy::Tidy::Topics { .. }))
        .map(|t| t.text())
}

/// What could be tidier in the notes (kernel/tidy.rs), as lines.
fn tidiness(project: &Project, index: &Index) -> Result<Vec<String>> {
    let mut notes: Vec<NoteFacts> = Vec::new();
    let mut st = index.db.prepare(
        "SELECT path, COALESCE(id, '') FROM notes WHERE kind = ?1 AND COALESCE(status, '') NOT IN ('archived', 'superseded') ORDER BY path",
    )?;
    let rows: Vec<(String, String)> =
        st.query_map([Kind::Note.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    for (path, id) in rows {
        notes.extend(facts_of(project, &id, &path));
    }
    if std::env::var_os("PG_EXPLAIN").is_some() {
        for n in &notes {
            if let Some(s) = tidy::spread(&n.pieces) {
                eprintln!("explain: {} parts alike {s:.3}", n.path);
            }
        }
    }
    Ok(tidy::tidy(&notes).iter().map(|t| t.text()).collect())
}

/// One sleep a project at a time.
struct SleepLock {
    _file: std::fs::File,
}

impl SleepLock {
    fn try_take(project: &Project) -> Result<Option<SleepLock>> {
        std::fs::create_dir_all(project.meta_dir())?;
        let f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(project.meta_dir().join("sleep.lock"))?;
        Ok(match f.try_lock() {
            Ok(()) => Some(SleepLock { _file: f }),
            Err(_) => None,
        })
    }
}
