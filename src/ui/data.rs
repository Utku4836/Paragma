//! What each part of the interface shows, read from memory, and the pg
//! commands its actions run. An action is a pg command (run as a child,
//! with the project named): the same checks, the same event log, the same
//! sync as when an agent runs it, and the agents see what was done.

use crate::note::Note;
use crate::project::{Home, Project, files, plan};
use crate::state::journal::{Did, Entry};
use std::path::{Path, PathBuf};

/// A pg command's outcome: whether it went well, what it said.
pub struct Ran {
    pub ok: bool,
    pub out: String,
}

/// Runs a pg command (this same program) for a project.
pub fn pg(exe: &Path, project: Option<&str>, args: &[&str]) -> Ran {
    let mut c = std::process::Command::new(exe);
    if let Some(p) = project {
        c.args(["--project", p]);
    }
    c.args(args).env("PARAGMA_SESSION", "interface").stdin(std::process::Stdio::null());
    match c.output() {
        Ok(o) => Ran {
            ok: o.status.success(),
            out: format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
                .trim()
                .to_string(),
        },
        Err(e) => Ran { ok: false, out: e.to_string() },
    }
}

/// The first line of what a command said, for the status bar.
pub fn first(out: &str) -> String {
    out.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim_start_matches("pg: ").to_string()
}

#[derive(Clone, Debug)]
pub struct ProjectRow {
    pub name: String,
    pub notes: usize,
    pub open: usize,
    pub last: String,
}

pub fn projects(home: &Home) -> Vec<ProjectRow> {
    home.projects()
        .into_iter()
        .map(|p| {
            let notes = crate::project::scan::notes(&p.dir);
            let note = plan_note(&p);
            let open = plan::steps(&note).iter().filter(|s| s.open()).count();
            let last = plan::handoffs(&note)
                .first()
                .map(|h| h.heading.split(" · ").next().unwrap_or("").to_string())
                .unwrap_or_default();
            ProjectRow { name: p.name.clone(), notes: notes.len(), open, last }
        })
        .collect()
}

pub fn plan_note(p: &Project) -> Note {
    files::read_text(&p.note_file(plan::FILE)).map(|t| Note::parse(&t)).unwrap_or_default()
}

/// A row of the notes tree.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow {
    /// Path in the project, with '/'; a folder ends in '/'.
    pub rel: String,
    pub depth: usize,
    pub folder: bool,
    pub title: String,
    /// The note's id and kind ("n4 note"), "" for a folder.
    pub tag: String,
    /// Notes below a folder.
    pub count: usize,
}

/// The project's folders and notes, the open folders' contents shown.
pub fn tree(p: &Project, open: &std::collections::HashSet<String>) -> Vec<TreeRow> {
    let notes = crate::project::scan::notes(&p.dir);
    let mut folders: Vec<String> = crate::project::scan::folders(&p.dir);
    folders.sort();
    let mut out = Vec::new();
    fn add(
        p: &Project,
        prefix: &str,
        depth: usize,
        folders: &[String],
        notes: &[crate::project::scan::FileState],
        open: &std::collections::HashSet<String>,
        out: &mut Vec<TreeRow>,
    ) {
        // folders right under prefix
        for f in folders
            .iter()
            .filter(|f| f.strip_prefix(prefix).is_some_and(|rest| !rest.is_empty() && !rest.contains('/')))
        {
            let rel = format!("{f}/");
            let count = notes.iter().filter(|n| n.rel.starts_with(&rel)).count();
            let name = f.rsplit('/').next().unwrap_or(f).to_string();
            out.push(TreeRow { rel: rel.clone(), depth, folder: true, title: name, tag: String::new(), count });
            if open.contains(&rel) {
                add(p, &rel, depth + 1, folders, notes, open, out);
            }
        }
        // the project note is the plan (its own section)
        for n in notes
            .iter()
            .filter(|n| n.rel != plan::FILE && n.rel.strip_prefix(prefix).is_some_and(|rest| !rest.contains('/')))
        {
            let text = files::read_text(&p.dir.join(&n.rel)).unwrap_or_default();
            let note = Note::parse(&text);
            let stem = n.rel.rsplit('/').next().unwrap_or(&n.rel).trim_end_matches(".md").to_string();
            let tag = format!("{} {}", note.id().unwrap_or("-"), note.kind().as_str());
            out.push(TreeRow {
                rel: n.rel.clone(),
                depth,
                folder: false,
                title: note.props.text("title").map(str::to_string).unwrap_or(stem),
                tag,
                count: 0,
            });
        }
    }
    let with_slash: Vec<String> = folders.iter().map(|f| f.trim_end_matches('/').to_string()).collect();
    add(p, "", 0, &with_slash, &notes, open, &mut out);
    out
}

/// A file kept with a note.
#[derive(Clone, Debug)]
pub struct FileRow {
    /// Path in the project.
    pub rel: String,
    pub note: String,
    pub size: u64,
    pub date: String,
    pub source: String,
    pub versions: usize,
}

pub fn kept_files(p: &Project) -> Vec<FileRow> {
    let mut out = Vec::new();
    for n in crate::project::scan::notes(&p.dir) {
        let text = files::read_text(&p.dir.join(&n.rel)).unwrap_or_default();
        if !text.contains("## Attachments") {
            continue;
        }
        let note = Note::parse(&text);
        let folder = Path::new(&n.rel).parent().map(|f| f.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        for c in crate::project::attach::cards(&note) {
            let rel = if folder.is_empty() { c.name.clone() } else { format!("{folder}/{}", c.name) };
            out.push(FileRow {
                rel,
                note: n.rel.trim_end_matches(".md").to_string(),
                size: c.size,
                date: c.date.clone(),
                source: c.source.clone(),
                versions: c.versions.len(),
            });
        }
    }
    out
}

/// A line of the activity: when, who, what; the number pg undo takes for
/// what pg did by itself.
#[derive(Clone, Debug)]
pub struct ActRow {
    pub at: i64,
    pub who: String,
    pub what: String,
    pub undo: Option<i64>,
    /// pg's own (a handoff it wrote, a draft): it can be taken back.
    pub auto: bool,
}

pub fn activity(p: &Project) -> Vec<ActRow> {
    let Ok(state) = crate::state::State::open(p) else { return Vec::new() };
    let now = crate::output::now();
    let journal = state.journal(now - 14 * 86_400).unwrap_or_default();
    let folders = state.folders(now - 30 * 86_400).unwrap_or_default();
    let undone: Vec<i64> =
        journal.iter().filter_map(|e| if let Did::Undone { of } = e.did { Some(of) } else { None }).collect();
    let mut out: Vec<ActRow> = journal
        .iter()
        .filter_map(|e| {
            let who = if e.conversation == "interface" {
                "you".to_string()
            } else {
                folders
                    .get(&e.conversation)
                    .filter(|f| !f.is_empty())
                    .cloned()
                    .unwrap_or_else(|| "a conversation".into())
            };
            let what = describe(e)?;
            let auto = matches!(e.did, Did::Auto { .. });
            Some(ActRow { at: e.at, who, what, undo: (auto && !undone.contains(&e.seq)).then_some(e.seq), auto })
        })
        .collect();
    out.reverse();
    out
}

/// What an event did, said for people; None for what only looked.
fn describe(e: &Entry) -> Option<String> {
    let s = |x: &str| Some(x.to_string());
    match &e.did {
        Did::Write { kind, id, what } => Some(match kind.as_str() {
            "attach" => format!("kept a file: {what}"),
            "confirm" => format!("confirmed {id}"),
            k if id.is_empty() => format!("wrote {k}"),
            k if k.contains('/') => format!("wrote {k} ({id})"),
            k => format!("wrote {k} {id} ({what})"),
        }),
        Did::Step { text } => Some(format!("added a step: {}", crate::output::one_line(text, 60))),
        Did::Finish { summary } => Some(format!("handed off: {}", crate::output::one_line(summary, 80))),
        Did::Mv { from, to } => Some(format!("moved {from} to {to}")),
        Did::Rm { what } => Some(format!("archived {what}")),
        Did::Edit { file } => Some(format!("edited {file}")),
        Did::Run { command, passed, failing } => Some(format!(
            "ran {}: {}",
            crate::output::one_line(command.rsplit(['/', '\\']).next().unwrap_or(command), 40),
            if *passed { "passed".to_string() } else { format!("{failing} failing") }
        )),
        Did::Told { text } => Some(format!("left a note: {}", crate::output::one_line(text, 70))),
        Did::Take { step } => Some(format!("took step {step}")),
        Did::Auto { .. } => Some(format!("pg by itself: {}", crate::cli::undo_describe(e))),
        Did::Undone { of } => Some(format!("took back {of}")),
        Did::Secret { title, .. } => Some(format!("kept a secret: {title}")),
        Did::Open => s("opened the project"),
        _ => None,
    }
}

/// Where the program itself is, to run pg commands.
pub fn exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("pg"))
}

/// Opens a file with the program the system has for it.
pub fn open_with_system(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd").args(["/C", "start", ""]).arg(path).spawn().map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn().map(|_| ())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn().map(|_| ())
    }
}
