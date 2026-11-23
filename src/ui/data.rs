//! What each part of the interface shows, read from memory, and the pg
//! commands its actions run. An action is a pg command (run as a child,
//! with the project named): the same checks, the same event log, the same
//! sync as when an agent runs it, and the agents see what was done.
//!
//! Notes are read once and kept by their time and size (`Cache`): a frame,
//! a reload or the live watch reads only what changed.

use crate::note::Note;
use crate::project::{Home, Project, files, plan};
use crate::state::journal::{Did, Entry};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A pg command's outcome: whether it went well, what it said.
#[derive(Clone, Debug)]
pub struct Ran {
    pub ok: bool,
    pub out: String,
}

/// Runs a pg command (this same program) for a project.
pub fn pg(exe: &Path, project: Option<&str>, args: &[&str]) -> Ran {
    pg_input(exe, project, args, None)
}

/// A pg command given a line on its input (a password, never on the
/// command line).
pub fn pg_input(exe: &Path, project: Option<&str>, args: &[&str], input: Option<&str>) -> Ran {
    let mut c = std::process::Command::new(exe);
    if let Some(p) = project {
        c.args(["--project", p]);
    }
    c.args(args).env("PARAGMA_SESSION", "interface");
    let run = match input {
        None => c.stdin(std::process::Stdio::null()).output(),
        Some(text) => (|| {
            use std::io::Write;
            let mut child = c
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?;
            if let Some(mut i) = child.stdin.take() {
                let _ = writeln!(i, "{text}");
            }
            child.wait_with_output()
        })(),
    };
    match run {
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

// ---------- notes, read once ----------

/// What the interface needs of a note.
#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub id: String,
    pub kind: String,
    pub title: Option<String>,
    pub due: Option<String>,
    /// The notes it links to ([[name]]), as written.
    pub links: Vec<String>,
    pub cards: Vec<crate::project::attach::Card>,
}

/// Notes by path, with the time and size they were read at.
#[derive(Default)]
pub struct Cache {
    notes: HashMap<PathBuf, (i64, u64, Meta)>,
}

impl Cache {
    /// The note's meta, read again only when the file changed.
    pub fn meta(&mut self, p: &Project, f: &crate::project::scan::FileState) -> Meta {
        let path = p.dir.join(&f.rel);
        if let Some((m, s, meta)) = self.notes.get(&path)
            && *m == f.mtime
            && *s == f.size
        {
            return meta.clone();
        }
        let text = files::read_text(&path).unwrap_or_default();
        let note = Note::parse(&text);
        let meta = Meta {
            id: note.id().unwrap_or("-").to_string(),
            kind: note.kind().as_str().to_string(),
            title: note.props.text("title").map(str::to_string),
            due: note.props.text("due").map(str::to_string),
            links: links_in(&note.body),
            cards: if text.contains("## Attachments") { crate::project::attach::cards(&note) } else { Vec::new() },
        };
        self.notes.insert(path, (f.mtime, f.size, meta.clone()));
        meta
    }
}

/// The [[links]] of a text: the name before any | or #.
pub fn links_in(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("[[") {
        let after = &rest[i + 2..];
        let Some(e) = after.find("]]") else { break };
        let inner = &after[..e];
        let name = inner.split(['|', '#']).next().unwrap_or("").trim();
        if !name.is_empty() && !name.contains('\n') && !out.iter().any(|x: &String| x == name) {
            out.push(name.to_string());
        }
        rest = &after[e + 2..];
    }
    out
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
    /// Changed here after the last sync: not sent yet.
    pub unsent: bool,
    /// The other computer's version of a note both changed (name.conflict.md).
    pub conflict: bool,
}

/// The conflict copy's note: "a/b.conflict.md" is of "a/b.md".
pub fn conflict_of(rel: &str) -> Option<String> {
    rel.strip_suffix(".conflict.md").map(|s| format!("{s}.md"))
}

/// The project's folders and notes, the open folders' contents shown.
/// `synced`: when the last sync ended well (0: not connected).
pub fn tree(p: &Project, open: &HashSet<String>, cache: &mut Cache, synced: i64) -> Vec<TreeRow> {
    let notes = crate::project::scan::notes(&p.dir);
    let mut folders: Vec<String> = crate::project::scan::folders(&p.dir);
    folders.sort();
    let with_slash: Vec<String> = folders.iter().map(|f| f.trim_end_matches('/').to_string()).collect();
    let mut out = Vec::new();
    let synced_ns = synced.saturating_mul(1_000_000_000);
    // depth first, folders before notes, as a file explorer shows them
    #[allow(clippy::too_many_arguments)]
    fn add(
        p: &Project,
        prefix: &str,
        depth: usize,
        folders: &[String],
        notes: &[crate::project::scan::FileState],
        open: &HashSet<String>,
        cache: &mut Cache,
        synced_ns: i64,
        out: &mut Vec<TreeRow>,
    ) {
        for f in folders
            .iter()
            .filter(|f| f.strip_prefix(prefix).is_some_and(|rest| !rest.is_empty() && !rest.contains('/')))
        {
            let rel = format!("{f}/");
            let inside: Vec<&crate::project::scan::FileState> =
                notes.iter().filter(|n| n.rel.starts_with(&rel)).collect();
            let name = f.rsplit('/').next().unwrap_or(f).to_string();
            out.push(TreeRow {
                rel: rel.clone(),
                depth,
                folder: true,
                title: name,
                tag: String::new(),
                count: inside.len(),
                unsent: synced_ns > 0 && inside.iter().any(|n| n.mtime > synced_ns),
                conflict: inside.iter().any(|n| conflict_of(&n.rel).is_some()),
            });
            if open.contains(&rel) {
                add(p, &rel, depth + 1, folders, notes, open, cache, synced_ns, out);
            }
        }
        // the project note is the plan (its own section)
        for n in notes
            .iter()
            .filter(|n| n.rel != plan::FILE && n.rel.strip_prefix(prefix).is_some_and(|rest| !rest.contains('/')))
        {
            let meta = cache.meta(p, n);
            let stem = n.rel.rsplit('/').next().unwrap_or(&n.rel).trim_end_matches(".md").to_string();
            out.push(TreeRow {
                rel: n.rel.clone(),
                depth,
                folder: false,
                title: meta.title.clone().unwrap_or(stem),
                tag: format!("{} {}", meta.id, meta.kind),
                count: 0,
                unsent: synced_ns > 0 && n.mtime > synced_ns,
                conflict: conflict_of(&n.rel).is_some(),
            });
        }
    }
    add(p, "", 0, &with_slash, &notes, open, cache, synced_ns, &mut out);
    out
}

/// Every note of the project: its path (no .md) and title, for the palette
/// and links.
pub fn all_notes(p: &Project, cache: &mut Cache) -> Vec<(String, String)> {
    crate::project::scan::notes(&p.dir)
        .iter()
        .filter(|n| n.rel != plan::FILE)
        .map(|n| {
            let m = cache.meta(p, n);
            let rel = n.rel.trim_end_matches(".md").to_string();
            let title = m.title.unwrap_or_else(|| rel.rsplit('/').next().unwrap_or(&rel).to_string());
            (rel, title)
        })
        .collect()
}

/// The notes that link to `rel` ([[its path]], [[its name]] or [[its
/// title]]), and the notes `rel` links to that exist.
pub fn links_of(p: &Project, cache: &mut Cache, rel: &str) -> (Vec<String>, Vec<String>) {
    let all = crate::project::scan::notes(&p.dir);
    let me = rel.trim_end_matches(".md");
    let name = me.rsplit('/').next().unwrap_or(me).to_lowercase();
    let mut title = String::new();
    let mut mine = Vec::new();
    let mut metas = Vec::with_capacity(all.len());
    for n in &all {
        let m = cache.meta(p, n);
        if n.rel == rel {
            title = m.title.clone().unwrap_or_default().to_lowercase();
            mine = m.links.clone();
        }
        metas.push((n.rel.trim_end_matches(".md").to_string(), m));
    }
    let names_me = |l: &str| {
        let l = l.trim_end_matches(".md").to_lowercase();
        l == me.to_lowercase() || l == name || (!title.is_empty() && l == title)
    };
    let from: Vec<String> =
        metas.iter().filter(|(r, m)| r != me && m.links.iter().any(|l| names_me(l))).map(|(r, _)| r.clone()).collect();
    // what it links to, found by path, name or title
    let mut to = Vec::new();
    for l in mine {
        let ll = l.trim_end_matches(".md").to_lowercase();
        if let Some((r, _)) = metas.iter().find(|(r, m)| {
            r.to_lowercase() == ll
                || r.rsplit('/').next().is_some_and(|n| n.to_lowercase() == ll)
                || m.title.as_deref().is_some_and(|t| t.to_lowercase() == ll)
        }) && !to.contains(r)
        {
            to.push(r.clone());
        }
    }
    (from, to)
}

/// Notes with a due date: path (no .md), the date, days left; soonest first.
pub fn due(p: &Project, cache: &mut Cache) -> Vec<(String, String, i64)> {
    let mut out: Vec<(String, String, i64)> = crate::project::scan::notes(&p.dir)
        .iter()
        .filter_map(|n| {
            let m = cache.meta(p, n);
            let d = m.due?;
            let left = crate::project::items::days_left(&d)?;
            Some((n.rel.trim_end_matches(".md").to_string(), d, left))
        })
        .collect();
    out.sort_by_key(|x| x.2);
    out
}

/// A signature of what the project's files and pg's state are now: when it
/// changes, the interface reads again (an agent wrote, a sync came in).
pub fn signature(p: &Project) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for n in crate::project::scan::notes(&p.dir) {
        (n.rel, n.mtime, n.size).hash(&mut h);
    }
    for f in crate::project::scan::folders(&p.dir) {
        f.hash(&mut h);
    }
    if let Ok(entries) = std::fs::read_dir(p.meta_dir()) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            // the index changes when pg reads; only state and notes matter
            if !name.starts_with("state") {
                continue;
            }
            if let Ok(m) = e.metadata() {
                name.hash(&mut h);
                m.len().hash(&mut h);
                m.modified().ok().hash(&mut h);
            }
        }
    }
    h.finish()
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

pub fn kept_files(p: &Project, cache: &mut Cache) -> Vec<FileRow> {
    let mut out = Vec::new();
    for n in crate::project::scan::notes(&p.dir) {
        let meta = cache.meta(p, &n);
        if meta.cards.is_empty() {
            continue;
        }
        let folder = Path::new(&n.rel).parent().map(|f| f.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        for c in meta.cards {
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

fn who(e: &Entry, folders: &HashMap<String, String>) -> String {
    if e.conversation == "interface" {
        "you".to_string()
    } else {
        folders.get(&e.conversation).filter(|f| !f.is_empty()).cloned().unwrap_or_else(|| "a conversation".into())
    }
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
            let what = describe(e)?;
            let auto = matches!(e.did, Did::Auto { .. });
            Some(ActRow {
                at: e.at,
                who: who(e, &folders),
                what,
                undo: (auto && !undone.contains(&e.seq)).then_some(e.seq),
                auto,
            })
        })
        .collect();
    out.reverse();
    out
}

/// A conversation that worked on the project lately.
#[derive(Clone, Debug)]
pub struct AgentRow {
    pub conversation: String,
    pub who: String,
    /// at work, handed off, stopped without a handoff
    pub state: AgentState,
    pub last: i64,
    /// What it did last (said for people).
    pub doing: String,
    /// Steps it has taken and not handed off.
    pub steps: Vec<usize>,
    /// What it did, newest first.
    pub did: Vec<(i64, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AgentState {
    AtWork,
    HandedOff,
    Stopped,
    Quiet,
}

/// The conversations of the last two days, the ones at work first.
pub fn agents(p: &Project) -> Vec<AgentRow> {
    let Ok(state) = crate::state::State::open(p) else { return Vec::new() };
    let now = crate::output::now();
    let journal = state.journal(now - 2 * 86_400).unwrap_or_default();
    let folders = state.folders(now - 30 * 86_400).unwrap_or_default();
    let mut by: Vec<AgentRow> = Vec::new();
    for e in &journal {
        if e.conversation == "interface" || e.conversation.is_empty() {
            continue;
        }
        let i = match by.iter().position(|a| a.conversation == e.conversation) {
            Some(i) => i,
            None => {
                by.push(AgentRow {
                    conversation: e.conversation.clone(),
                    who: who(e, &folders),
                    state: AgentState::Quiet,
                    last: 0,
                    doing: String::new(),
                    steps: Vec::new(),
                    did: Vec::new(),
                });
                by.len() - 1
            }
        };
        let a = &mut by[i];
        a.last = e.at;
        match &e.did {
            Did::Take { step } if *step > 0 && !a.steps.contains(step) => a.steps.push(*step),
            Did::Finish { .. } => a.steps.clear(),
            _ => {}
        }
        if let Some(d) = describe(e) {
            a.doing = d.clone();
            a.did.push((e.at, d));
        }
        // what it is now: handed off last, else changed things since
        a.state = match (&e.did, a.state) {
            (Did::Finish { .. }, _) => AgentState::HandedOff,
            (d, _) if d.changes() => AgentState::Stopped,
            (_, AgentState::HandedOff) => AgentState::HandedOff,
            (_, AgentState::Stopped) => AgentState::Stopped,
            _ => AgentState::Quiet,
        };
    }
    for a in &mut by {
        if a.state != AgentState::HandedOff && now - a.last < crate::kernel::experts::QUIET {
            a.state = AgentState::AtWork;
        }
        if a.state == AgentState::HandedOff || now - a.last >= crate::kernel::experts::HOLD {
            a.steps.clear();
        }
        a.did.reverse();
    }
    let rank = |s: AgentState| match s {
        AgentState::AtWork => 0,
        AgentState::Stopped => 1,
        AgentState::HandedOff => 2,
        AgentState::Quiet => 3,
    };
    by.sort_by(|a, b| rank(a.state).cmp(&rank(b.state)).then(b.last.cmp(&a.last)));
    by
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

// ---------- setup: the agents on this computer ----------

/// A row of the Setup section.
#[derive(Clone, Debug)]
pub enum SetupRow {
    /// Bring everything pg set up to this version.
    UpdateAll,
    Head(&'static str),
    Agent {
        id: &'static str,
        name: &'static str,
        found: Vec<&'static str>,
        installed: bool,
        instructions: bool,
        /// None: this agent has no hooks.
        hooks: Option<bool>,
        place: String,
    },
    Mcp {
        id: &'static str,
        name: &'static str,
        on: bool,
    },
    Doctor,
}

impl SetupRow {
    /// Rows a key does something on.
    pub fn acts(&self) -> bool {
        !matches!(self, SetupRow::Head(_))
    }
}

/// What is set up for each agent, read from the agents' own files.
pub fn setup_rows() -> Vec<SetupRow> {
    let mut out = vec![SetupRow::UpdateAll, SetupRow::Head("Agents: instructions in every conversation, hooks")];
    for a in crate::agents::known() {
        let hooks = crate::agents::hooks::AGENTS
            .contains(&a.id)
            .then(|| crate::agents::hooks::installed(&crate::agents::hooks::file(a.id, None)));
        out.push(SetupRow::Agent {
            id: a.id,
            name: a.name,
            found: a.apps.clone(),
            installed: a.installed,
            instructions: crate::skills::installed(&a.global),
            hooks,
            place: crate::agents::short(&a.global),
        });
    }
    let apps: Vec<crate::agents::mcp::Client> =
        crate::agents::mcp::clients().into_iter().filter(|c| c.installed).collect();
    if !apps.is_empty() {
        out.push(SetupRow::Head("Apps: Paragma as an MCP server"));
        for c in apps {
            out.push(SetupRow::Mcp { id: c.id, name: c.name, on: crate::agents::mcp::installed(&c) });
        }
    }
    out.push(SetupRow::Head("This project"));
    out.push(SetupRow::Doctor);
    out
}

// ---------- two versions of a note side by side ----------

/// Joins two versions of a text line by line: the lines both have once,
/// where they differ both kept between markers (as git marks a clash).
pub fn two_way(mine: &str, theirs: &str, mine_name: &str, theirs_name: &str) -> (String, usize) {
    let a: Vec<&str> = mine.lines().collect();
    let b: Vec<&str> = theirs.lines().collect();
    // the longest common lines (small notes: the plain table is fine)
    let (n, m) = (a.len(), b.len());
    if n * m > 4_000_000 {
        return (format!("<<<<<<< {mine_name}\n{mine}\n=======\n{theirs}\n>>>>>>> {theirs_name}\n"), 1);
    }
    let mut l = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            l[i][j] = if a[i] == b[j] { l[i + 1][j + 1] + 1 } else { l[i + 1][j].max(l[i][j + 1]) };
        }
    }
    let mut out = String::new();
    let mut clashes = 0;
    let (mut i, mut j) = (0, 0);
    let mut ours: Vec<&str> = Vec::new();
    let mut other: Vec<&str> = Vec::new();
    let mut flush = |ours: &mut Vec<&str>, other: &mut Vec<&str>, out: &mut String| {
        if ours.is_empty() && other.is_empty() {
            return;
        }
        // only one side added lines: no clash, both kept
        if ours.is_empty() || other.is_empty() {
            for x in ours.iter().chain(other.iter()) {
                out.push_str(x);
                out.push('\n');
            }
        } else {
            clashes += 1;
            out.push_str(&format!("<<<<<<< {mine_name}\n"));
            for x in ours.iter() {
                out.push_str(x);
                out.push('\n');
            }
            out.push_str("=======\n");
            for x in other.iter() {
                out.push_str(x);
                out.push('\n');
            }
            out.push_str(&format!(">>>>>>> {theirs_name}\n"));
        }
        ours.clear();
        other.clear();
    };
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            flush(&mut ours, &mut other, &mut out);
            out.push_str(a[i]);
            out.push('\n');
            i += 1;
            j += 1;
        } else if j >= m || (i < n && l[i + 1][j] >= l[i][j + 1]) {
            ours.push(a[i]);
            i += 1;
        } else {
            other.push(b[j]);
            j += 1;
        }
    }
    flush(&mut ours, &mut other, &mut out);
    (out, clashes)
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

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/ui/data.rs"]
mod tests;
