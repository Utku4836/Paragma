//! The interface's state and what keys and clicks do. Drawing is in
//! ui/draw.rs; what is shown comes from ui/data.rs; the Home, Agents,
//! Sync and Setup sections, the palette, the search and the work done in
//! the background are in ui/panels.rs.

use super::data::{self, ActRow, AgentRow, FileRow, ProjectRow, SetupRow, TreeRow};
use super::editor::{Editor, Pos};
use super::theme::mark;
use crate::project::{Home, Project};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Home,
    Projects,
    Notes,
    Plan,
    Files,
    Agents,
    Activity,
    Sync,
    Setup,
}

/// How many sections there are.
pub const N: usize = 9;

pub const SECTIONS: [(Section, &str, &str); N] = [
    (Section::Home, mark::HOME, "Home"),
    (Section::Projects, mark::PROJECTS, "Projects"),
    (Section::Notes, mark::NOTES, "Notes"),
    (Section::Plan, mark::PLAN, "Plan"),
    (Section::Files, mark::FILES, "Files"),
    (Section::Agents, mark::AGENTS, "Agents"),
    (Section::Activity, mark::ACTIVITY, "Activity"),
    (Section::Sync, mark::SYNC, "Sync"),
    (Section::Setup, mark::SETUP, "Setup"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Main,
}

/// What a typed answer is for.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    Goal,
    NewNote,
    NewFolder,
    Move(String),
    NewProject,
    AddStep,
    EditStep(usize),
    Answer(usize),
    Question,
    Attach,
    AttachTo(String),
    /// A note to every conversation here (pg tell).
    Tell,
    /// The name of a new private repository; moving the memory there or not.
    NewRepo {
        moving: bool,
    },
    /// The address of a place: a git repository's, a folder's path, a
    /// WebDAV address.
    Place {
        kind: String,
        moving: bool,
    },
    /// WebDAV: the user name, then the password (shown as dots).
    DavUser,
    DavPassword,
    /// Where a backup goes; where the daily ones go; a backup's zip.
    BackupFolder,
    DailyFolder,
    RestorePath,
}

/// What a yes is for.
#[derive(Clone, Debug, PartialEq)]
pub enum Confirm {
    Archive(String),
    DropStep(usize),
    CloseUnsaved,
    QuitUnsaved,
    FilesOn,
    Undo(i64),
    /// A note (or folder) dragged onto a folder.
    MoveInto(String, String),
    Disconnect,
}

/// What a choice in a window does.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Nothing,
    /// A pg command in the background; for the project shown or not; what
    /// is done with what it said.
    Pg {
        args: Vec<String>,
        project: bool,
        then: Then,
    },
    /// Sign in to GitHub (the terminal is GitHub's meanwhile), then list
    /// the repositories (to move the memory to one, or to connect).
    SignIn {
        moving: bool,
    },
    /// The repositories to choose from (to connect, or to move to).
    Repos {
        moving: bool,
    },
    /// Text put in the note as it is (a dropped path, pasted after all).
    Insert(String),
    /// Files kept with a note (dropped on the window).
    Attach {
        note: String,
        paths: Vec<String>,
    },
    /// Connecting or moving to a kind of place: github, git, a cloud's id,
    /// folder, webdav.
    Place {
        kind: String,
        moving: bool,
    },
    /// The .env files go to the new place, or not; then the old place
    /// emptied, or not (a transfer).
    PlaceEnv(bool),
    PlaceOld(bool),
    /// A backup now with the .env files or without; the daily ones too.
    BackupEnv(bool),
    DailyEnv(bool),
    /// A backup chosen to restore; then joined with the memory, or put in
    /// its place.
    RestoreFrom(String),
    RestoreMode(String, bool),
    /// A note both computers changed: merged in the editor, this one kept,
    /// or the other one taken. The note and its copy (name.conflict.md).
    ResolveMerge(String, String),
    ResolveKeep(String),
    ResolveTake(String, String),
}

/// What is done with a background pg command's answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Then {
    /// Its first line in the status bar.
    Status,
    /// All of it in a window with this title.
    Message(String),
    /// Its first line, then the agents' setup read again.
    Setup,
    /// All of it in a window; the Sync section back to its settings.
    Synced,
}

/// An entry of the palette (Ctrl+K).
#[derive(Clone, Debug, PartialEq)]
pub struct PItem {
    pub label: String,
    pub hint: String,
    pub act: PAct,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PAct {
    Section(Section),
    /// A note, by its path without .md.
    Note(String),
    Cmd(&'static str),
    /// Search the memory for the text.
    Search(String),
}

/// A note found by the search.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// Its path, without .md.
    pub path: String,
    pub tag: String,
    pub heading: String,
    pub snippet: String,
}

#[derive(Clone, Debug)]
pub enum Modal {
    Input {
        ask: Ask,
        title: String,
        text: String,
    },
    Confirm {
        what: Confirm,
        title: String,
        lines: Vec<String>,
    },
    /// Several ways to go on; the first is the usual one.
    Choose {
        title: String,
        lines: Vec<String>,
        options: Vec<(String, Act)>,
        cur: usize,
    },
    /// The note changed outside the editor (an agent): what to do.
    Changed {
        theirs: String,
    },
    /// Lines to read, closed by any key.
    Message {
        title: String,
        lines: Vec<String>,
    },
    /// Everything there is to do and go to, by typing a part of its name.
    Palette {
        text: String,
        items: Vec<PItem>,
        cur: usize,
    },
    /// The memory searched as the words are typed.
    Search {
        text: String,
        results: Vec<Found>,
        cur: usize,
        /// "3 notes", "nothing on it", "searching…"
        note: String,
    },
    Help,
}

/// A note open in the editor.
pub struct Open {
    pub project: String,
    pub rel: String,
    pub ed: Editor,
    /// The note's text when it was opened (or saved): what an outside
    /// change is told against.
    pub base: String,
    /// Reading (Markdown shown) instead of editing.
    pub reading: bool,
    /// The search being typed (Ctrl+F), and the last one.
    pub find: Option<String>,
    pub last_find: String,
    /// Notes offered for a [[link]] being typed, and the chosen one.
    pub links: Vec<String>,
    pub link_cur: usize,
    /// Changed outside meanwhile (seen by the watch).
    pub outside: bool,
    pub checked: Instant,
    /// Merging the other computer's copy (name.conflict.md): taken out
    /// once this is saved.
    pub resolving: Option<String>,
}

/// A place on the screen a click means something, as the last frame drew it.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    Section(usize),
    Row(usize),
    Button(String),
    Editor,
    /// The note shown beside the tree: the wheel scrolls it.
    Preview,
    ModalButton(usize),
    /// A tab of an open note, and its ×.
    Tab(usize),
    TabClose(usize),
}

/// A row of the Home section.
#[derive(Clone, Debug, PartialEq)]
pub enum HomeRow {
    Title(String),
    Line(String),
    Blank,
    Goal(String),
    Step(usize, String, String),
    Due(String, String, i64),
    Recent(String),
    Agent(String, String),
    Sync(String, bool),
    Handoff(String, String),
    Projects,
}

impl HomeRow {
    pub fn acts(&self) -> bool {
        !matches!(self, HomeRow::Title(_) | HomeRow::Line(_) | HomeRow::Blank)
    }
}

/// A row of the Sync section, by its parts: the status, where the memory
/// goes, what goes with it, backups, notes changed on two computers.
#[derive(Clone, Debug, PartialEq)]
pub enum SyncRow {
    Head(&'static str),
    /// A line to read; red when something is wrong.
    Info(String, bool),
    SyncNow,
    Connect,
    Transfer,
    Disconnect,
    Env(bool),
    Files(bool),
    Limit(u64),
    BackupNow,
    /// The daily backup: on or off, and its folder.
    Daily(bool, String),
    Restore,
    /// A note both computers changed: its copy.
    Conflict(String),
    /// The repositories to choose from: a new private one first.
    NewRepo,
    Repo(crate::sync::github::Repo),
}

impl SyncRow {
    pub fn acts(&self) -> bool {
        !matches!(self, SyncRow::Head(_) | SyncRow::Info(..))
    }
}

/// A connection or a transfer being chosen, step by step: where (pg sync's
/// words for it), whether the .env files go, whether the old place is
/// emptied, a WebDAV user and password.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pending {
    pub moving: bool,
    pub target: Vec<String>,
    pub env: bool,
    pub user: String,
    pub password: String,
    /// A backup's folder, waiting for the .env answer.
    pub folder: String,
}

/// What the Sync section shows: the settings, or the repositories.
#[derive(Clone, Debug, PartialEq)]
pub enum SyncView {
    Main,
    Repos { login: String, repos: Vec<crate::sync::github::Repo>, filter: String, moving: bool },
}

/// Something to do with the terminal given away meanwhile.
#[derive(Clone, Debug, PartialEq)]
pub enum Outside {
    /// Signing in to GitHub; then the repositories, to move to or not.
    SignIn(bool),
}

/// Work done in the background: what it is, and its answer when it ends.
pub struct Job {
    pub what: String,
    pub rx: std::sync::mpsc::Receiver<JobDone>,
}

pub enum JobDone {
    Ran(data::Ran, Then),
    /// The GitHub account, its repositories, and a memory found in one.
    Account(Result<Option<Accounted>, String>, bool),
}

pub struct Accounted {
    pub login: String,
    pub repos: Vec<crate::sync::github::Repo>,
    pub found: Option<(crate::sync::github::Repo, String, String)>,
}

pub struct App {
    pub home: Home,
    pub exe: PathBuf,
    pub section: Section,
    pub focus: Focus,
    pub project: Option<Project>,
    pub projects: Vec<ProjectRow>,
    pub tree: Vec<TreeRow>,
    pub expanded: HashSet<String>,
    pub files: Vec<FileRow>,
    pub activity: Vec<ActRow>,
    pub agents: Vec<AgentRow>,
    pub setup: Vec<SetupRow>,
    /// The chosen row of each section's list.
    pub cur: [usize; N],
    /// The first row shown of each section's list.
    pub top: [usize; N],
    pub open: Option<Open>,
    /// The other notes open, behind the one shown, and the tabs' order
    /// (project, path).
    pub tabs: Vec<Open>,
    pub tab_order: Vec<(String, String)>,
    pub modal: Option<Modal>,
    pub status: Option<(String, bool, Instant)>,
    pub clipboard: Option<arboard::Clipboard>,
    /// Kept when the system clipboard cannot be reached.
    pub copied: String,
    pub hits: Vec<(Rect, Hit)>,
    pub last_click: Option<(Instant, u16, u16)>,
    pub dragging: bool,
    /// A row of the notes tree being dragged, and the row it is over.
    pub drag_from: Option<usize>,
    pub drag_over: Option<usize>,
    pub quit: bool,
    /// Something outside the interface (signing in to GitHub), with the
    /// screen given to it meanwhile.
    pub run_outside: Option<Outside>,
    /// The editor's text area.
    pub ed_area: Rect,
    /// A file's path typed in the Files section, waiting for its note.
    pub pending_path: String,
    /// Where the mouse is: the row under it is lit.
    pub hover: Option<(u16, u16)>,
    /// How far the preview is scrolled, and the note it is of.
    pub preview_scroll: usize,
    pub preview_of: String,
    /// Notes read once, by their time and size.
    pub cache: data::Cache,
    /// The notes opened last in this project, the last first.
    pub recent: Vec<String>,
    pub sync_view: SyncView,
    /// A connection, transfer or backup being chosen.
    pub pending: Pending,
    pub jobs: Vec<Job>,
    /// The search index, opened once for the project searched.
    pub index: Option<(String, crate::index::Index)>,
    /// When the search typed is due to run.
    pub search_due: Option<Instant>,
    /// The project's files and state as last seen: a change reads again.
    pub sig: u64,
    pub watched: Instant,
    /// Notes changed here after the last sync.
    pub unsent: usize,
    /// The Home section's rows, made at each reload.
    pub home_list: Vec<HomeRow>,
    /// The other computer's copies of notes here (name.conflict.md).
    pub conflicts: Vec<String>,
    /// The notes linking to the note previewed, and those it links to.
    pub links_for: Option<(String, Vec<String>, Vec<String>)>,
}

pub fn index(s: Section) -> usize {
    SECTIONS.iter().position(|x| x.0 == s).unwrap_or(0)
}

impl App {
    pub fn new(home: Home, project: Option<Project>) -> App {
        let mut app = App {
            home,
            exe: data::exe(),
            section: if project.is_some() { Section::Home } else { Section::Projects },
            focus: Focus::Main,
            project,
            projects: Vec::new(),
            tree: Vec::new(),
            expanded: HashSet::new(),
            files: Vec::new(),
            activity: Vec::new(),
            agents: Vec::new(),
            setup: Vec::new(),
            cur: [0; N],
            top: [0; N],
            open: None,
            tabs: Vec::new(),
            tab_order: Vec::new(),
            modal: None,
            status: None,
            clipboard: arboard::Clipboard::new().ok(),
            copied: String::new(),
            hits: Vec::new(),
            last_click: None,
            dragging: false,
            drag_from: None,
            drag_over: None,
            quit: false,
            run_outside: None,
            ed_area: Rect::default(),
            pending_path: String::new(),
            hover: None,
            preview_scroll: 0,
            preview_of: String::new(),
            cache: data::Cache::default(),
            recent: Vec::new(),
            sync_view: SyncView::Main,
            pending: Pending::default(),
            jobs: Vec::new(),
            index: None,
            search_due: None,
            sig: 0,
            watched: Instant::now(),
            unsent: 0,
            home_list: Vec::new(),
            conflicts: Vec::new(),
            links_for: None,
        };
        app.projects = data::projects(&app.home);
        app.open_small_trees();
        app.load_recent();
        app.reload();
        app.move_cur(0);
        app
    }

    pub fn say(&mut self, text: impl Into<String>, bad: bool) {
        self.status = Some((text.into(), bad, Instant::now()));
    }

    pub fn project_name(&self) -> Option<String> {
        self.project.as_ref().map(|p| p.name.clone())
    }

    /// A project with few notes shows them all at once: its folders open.
    fn open_small_trees(&mut self) {
        if let Some(p) = &self.project {
            let notes = crate::project::scan::notes(&p.dir);
            if notes.len() <= 40 {
                for f in crate::project::scan::folders(&p.dir) {
                    self.expanded.insert(format!("{}/", f.trim_end_matches('/')));
                }
            }
        }
    }

    /// Reads again what the sections show (the notes only where they changed).
    pub fn reload(&mut self) {
        if self.projects.is_empty() || matches!(self.section, Section::Projects | Section::Home) {
            self.projects = data::projects(&self.home);
        }
        if let Some(p) = &self.project {
            self.project = self.home.project(&p.name);
        }
        let synced = match crate::sync::Config::load(&self.home) {
            Some(_) => crate::sync::Status::load(&self.home).synced.max(1),
            None => 0,
        };
        match self.project.clone() {
            Some(p) => {
                self.tree = data::tree(&p, &self.expanded, &mut self.cache, synced);
                self.files = data::kept_files(&p, &mut self.cache);
                self.activity = data::activity(&p);
                self.agents = data::agents(&p);
                let ns = synced.saturating_mul(1_000_000_000);
                let notes = crate::project::scan::notes(&p.dir);
                self.unsent = if synced > 0 { notes.iter().filter(|n| n.mtime > ns).count() } else { 0 };
                self.conflicts =
                    notes.into_iter().filter(|n| data::conflict_of(&n.rel).is_some()).map(|n| n.rel).collect();
                self.sig = data::signature(&p);
            }
            None => {
                self.tree.clear();
                self.files.clear();
                self.activity.clear();
                self.agents.clear();
                self.unsent = 0;
                self.conflicts.clear();
            }
        }
        self.home_list = self.home_rows();
        self.links_for = None;
        if self.section == Section::Setup || self.setup.is_empty() {
            self.setup = data::setup_rows();
        }
        for (i, (s, _, _)) in SECTIONS.iter().enumerate() {
            let n = self.rows(*s).max(1);
            if self.cur[i] >= n {
                self.cur[i] = n - 1;
            }
        }
    }

    /// Rows of a section's list.
    pub fn rows(&self, s: Section) -> usize {
        match s {
            Section::Home => self.home_list.len(),
            Section::Projects => self.projects.len(),
            Section::Notes => self.tree.len(),
            Section::Plan => self.plan_rows().len(),
            Section::Files => self.files.len(),
            Section::Agents => self.agents.len() + 1,
            Section::Activity => self.activity.len(),
            Section::Sync => self.sync_rows().len(),
            Section::Setup => self.setup.len(),
        }
    }

    pub fn cur(&self) -> usize {
        self.cur[index(self.section)]
    }

    pub fn set_cur(&mut self, v: usize) {
        let n = self.rows(self.section);
        self.cur[index(self.section)] = if n == 0 { 0 } else { v.min(n - 1) };
    }

    /// Whether a key does something on this row (headings are passed over).
    fn acts(&self, row: usize) -> bool {
        match self.section {
            Section::Home => self.home_list.get(row).is_some_and(HomeRow::acts),
            Section::Setup => self.setup.get(row).is_some_and(SetupRow::acts),
            Section::Sync => self.sync_rows().get(row).is_some_and(SyncRow::acts),
            _ => true,
        }
    }

    /// Moves the cursor by `by` rows, over the rows nothing is done on.
    pub fn move_cur(&mut self, by: isize) {
        let n = self.rows(self.section);
        if n == 0 {
            return;
        }
        let start = self.cur() as isize;
        let mut at = (start + by).clamp(0, n as isize - 1);
        let step = if by < 0 { -1 } else { 1 };
        while !self.acts(at as usize) {
            let next = at + step;
            if next < 0 || next >= n as isize {
                // none that way: the nearest the other way
                let mut back = at;
                while back >= 0 && back < n as isize && !self.acts(back as usize) {
                    back -= step;
                }
                at = if back >= 0 && back < n as isize { back } else { start.clamp(0, n as isize - 1) };
                break;
            }
            at = next;
        }
        self.set_cur(at as usize);
    }

    pub fn go(&mut self, s: Section) {
        self.section = s;
        self.focus = Focus::Main;
        if s == Section::Setup || s == Section::Projects || s == Section::Home {
            self.reload();
        }
        self.move_cur(0);
    }

    // ---------- the plan's rows ----------

    pub fn plan_rows(&self) -> Vec<PlanRow> {
        let Some(p) = &self.project else { return Vec::new() };
        let note = data::plan_note(p);
        let mut out = vec![PlanRow::Goal(crate::project::plan::goal(&note))];
        for (i, s) in crate::project::plan::steps(&note).into_iter().enumerate() {
            if !s.dropped {
                out.push(PlanRow::Step(i + 1, s));
            }
        }
        for (i, q) in crate::project::plan::questions(&note).into_iter().enumerate() {
            out.push(PlanRow::Question(i + 1, q));
        }
        for h in crate::project::plan::handoffs(&note).into_iter().take(5) {
            out.push(PlanRow::Handoff(h));
        }
        out
    }

    // ---------- keys ----------

    pub fn key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if self.modal.is_some() {
            self.modal_key(k);
            return;
        }
        // everywhere: the palette, and the tabs of the notes open
        match k.code {
            KeyCode::Char('k') if ctrl => return self.palette(),
            KeyCode::PageDown if ctrl => return self.next_tab(1),
            KeyCode::PageUp if ctrl => return self.next_tab(-1),
            _ => {}
        }
        if self.open.is_some() {
            self.editor_key(k);
            return;
        }
        if self.section == Section::Sync && self.sync_typing(&k) {
            return;
        }
        match k.code {
            KeyCode::Char('q') if !ctrl => self.quit = true,
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('?') | KeyCode::F(1) => self.modal = Some(Modal::Help),
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = if self.focus == Focus::Sidebar { Focus::Main } else { Focus::Sidebar }
            }
            KeyCode::Char(c @ '1'..='9') => self.go(SECTIONS[(c as u8 - b'1') as usize].0),
            KeyCode::Char('/') => self.search(""),
            KeyCode::F(5) => {
                self.reload();
                self.say("read again", false);
            }
            KeyCode::Up if self.focus == Focus::Sidebar => {
                let i = index(self.section);
                self.section = SECTIONS[(i + N - 1) % N].0;
                self.move_cur(0);
            }
            KeyCode::Down if self.focus == Focus::Sidebar => {
                let i = index(self.section);
                self.section = SECTIONS[(i + 1) % N].0;
                self.move_cur(0);
            }
            KeyCode::Enter | KeyCode::Right if self.focus == Focus::Sidebar => self.go(self.section),
            KeyCode::Left | KeyCode::Esc if self.focus == Focus::Main && !self.in_tree_folder_left(&k) => {
                if self.section == Section::Sync && self.sync_view != SyncView::Main {
                    self.sync_view = SyncView::Main;
                    self.set_cur(0);
                } else {
                    self.focus = Focus::Sidebar
                }
            }
            KeyCode::Up => self.move_cur(-1),
            KeyCode::Down => self.move_cur(1),
            KeyCode::PageUp => self.move_cur(-10),
            KeyCode::PageDown => self.move_cur(10),
            KeyCode::Home => {
                self.set_cur(0);
                self.move_cur(0);
            }
            KeyCode::End => {
                self.set_cur(usize::MAX);
                self.move_cur(0);
            }
            _ => self.section_key(k),
        }
    }

    /// Left in the notes tree closes the open folder the cursor is in.
    fn in_tree_folder_left(&mut self, k: &KeyEvent) -> bool {
        if self.section != Section::Notes || k.code != KeyCode::Left {
            return false;
        }
        let Some(row) = self.tree.get(self.cur()).cloned() else { return false };
        if row.folder && self.expanded.remove(&row.rel) {
            self.reload();
            return true;
        }
        // to its folder
        if let Some((parent, _)) = row.rel.trim_end_matches('/').rsplit_once('/') {
            let parent = format!("{parent}/");
            if let Some(i) = self.tree.iter().position(|r| r.rel == parent) {
                self.set_cur(i);
                return true;
            }
        }
        false
    }

    pub fn ask(&mut self, ask: Ask, title: &str, text: &str) {
        self.modal = Some(Modal::Input { ask, title: title.into(), text: text.into() });
    }

    pub fn confirm(&mut self, what: Confirm, title: &str, lines: Vec<String>) {
        self.modal = Some(Modal::Confirm { what, title: title.into(), lines });
    }

    pub fn choose(&mut self, title: &str, lines: Vec<String>, options: Vec<(&str, Act)>) {
        self.modal = Some(Modal::Choose {
            title: title.into(),
            lines,
            options: options.into_iter().map(|(l, a)| (l.to_string(), a)).collect(),
            cur: 0,
        });
    }

    /// Keys of the section shown.
    pub fn section_key(&mut self, k: KeyEvent) {
        let c = match k.code {
            KeyCode::Char(c) => c,
            KeyCode::Enter => '\n',
            KeyCode::Right => '→',
            KeyCode::Delete => 'd',
            _ => return,
        };
        let row = self.cur();
        match self.section {
            Section::Home => self.home_key(c, row),
            Section::Projects => match c {
                '\n' | '→' => self.choose_project(row),
                'n' => self.ask(Ask::NewProject, "New project: its name", ""),
                _ => {}
            },
            Section::Notes => {
                let Some(r) = self.tree.get(row).cloned() else {
                    if c == 'n' {
                        self.ask(Ask::NewNote, "New note (folder/name)", "");
                    }
                    if c == 'N' {
                        self.ask(Ask::NewFolder, "New folder", "");
                    }
                    return;
                };
                let here = if r.folder { r.rel.clone() } else { parent(&r.rel) };
                match c {
                    '\n' | '→' if r.folder => {
                        if !self.expanded.remove(&r.rel) {
                            self.expanded.insert(r.rel.clone());
                        }
                        self.reload();
                    }
                    '\n' | '→' if r.conflict => self.resolve(&r.rel),
                    '\n' => self.open_note(&r.rel, false),
                    '→' | 'v' => self.open_note(&r.rel, true),
                    'e' => self.open_note(&r.rel, false),
                    'n' => self.ask(Ask::NewNote, "New note (folder/name)", &here),
                    'N' => self.ask(Ask::NewFolder, "New folder", &here),
                    'm' | 'r' => {
                        let from = r.rel.trim_end_matches(".md").trim_end_matches('/').to_string();
                        self.ask(Ask::Move(from.clone()), "Rename or move to", &from)
                    }
                    'd' => self.confirm(
                        Confirm::Archive(r.rel.clone()),
                        &format!("Archive {}?", r.rel.trim_end_matches(".md")),
                        vec!["It goes to .archive/ (pg mv brings it back).".into()],
                    ),
                    'a' if !r.folder => self.ask(
                        Ask::AttachTo(r.rel.trim_end_matches(".md").to_string()),
                        "Keep a file with this note: its path (or drop the file on the window)",
                        "",
                    ),
                    _ => {}
                }
            }
            Section::Plan => self.plan_key(c, row),
            Section::Files => match c {
                '\n' => {
                    if let (Some(f), Some(p)) = (self.files.get(row), &self.project) {
                        let path = p.dir.join(&f.rel);
                        match data::open_with_system(&path) {
                            Ok(()) => self.say(format!("opened {}", f.rel), false),
                            Err(e) => self.say(e.to_string(), true),
                        }
                    }
                }
                'a' => self.ask(Ask::Attach, "Keep a file: its path (or drop the file on the window)", ""),
                'g' => {
                    if let Some(f) = self.files.get(row).cloned() {
                        self.section = Section::Notes;
                        self.reveal(&format!("{}.md", f.note));
                    }
                }
                _ => {}
            },
            Section::Agents => self.agents_key(c, row),
            Section::Activity => {
                if c == 'u'
                    && let Some(n) = self.activity.get(row).and_then(|a| a.undo)
                {
                    let what = self.activity[row].what.clone();
                    self.confirm(Confirm::Undo(n), "Take it back?", vec![what]);
                }
            }
            Section::Sync => {
                if c == '\n' {
                    self.sync_action(row);
                }
            }
            Section::Setup => self.setup_key(c, row),
        }
    }

    fn plan_key(&mut self, c: char, row: usize) {
        let rows = self.plan_rows();
        let Some(r) = rows.get(row).cloned() else { return };
        let pg = |args: &[&str]| Act::Pg {
            args: args.iter().map(|s| s.to_string()).collect(),
            project: true,
            then: Then::Status,
        };
        match (r, c) {
            (_, 'a') => self.ask(Ask::AddStep, "New step", ""),
            (_, '?') => self.ask(Ask::Question, "A question for later", ""),
            (PlanRow::Step(n, s), ' ' | '\n' | 'x') => {
                let flag = if s.done { "--reopen" } else { "--done" };
                self.act(pg(&["steps", flag, &n.to_string()]));
            }
            (PlanRow::Step(n, s), 'l') => {
                let flag = if s.deferred { "--resume" } else { "--defer" };
                self.act(pg(&["steps", flag, &n.to_string()]));
            }
            (PlanRow::Step(n, s), 'e') => self.ask(Ask::EditStep(n), "Step", &s.text),
            (PlanRow::Step(n, s), 'd') => self.confirm(
                Confirm::DropStep(n),
                &format!("Take step {n} out of the plan?"),
                vec![s.text.clone(), "The other steps keep their numbers.".into()],
            ),
            (PlanRow::Question(n, q), '\n') if q.answer.is_none() => self.ask(Ask::Answer(n), &q.text, ""),
            (PlanRow::Goal(g), '\n' | 'e') => self.ask(Ask::Goal, "The project's goal", &g),
            _ => {}
        }
    }

    pub fn choose_project(&mut self, row: usize) {
        let Some(name) = self.projects.get(row).map(|p| p.name.clone()) else { return };
        self.project = self.home.project(&name);
        self.expanded.clear();
        self.open_small_trees();
        for s in [Section::Home, Section::Notes, Section::Plan, Section::Files, Section::Agents, Section::Activity] {
            self.cur[index(s)] = 0;
            self.top[index(s)] = 0;
        }
        self.index = None;
        self.load_recent();
        self.reload();
        self.go(Section::Home);
        self.say(format!("project {name}"), false);
    }

    /// Opens the folders down to a note and puts the cursor on it.
    pub fn reveal(&mut self, rel: &str) {
        let mut acc = String::new();
        for part in rel.split('/').collect::<Vec<_>>().iter().rev().skip(1).rev() {
            acc.push_str(part);
            acc.push('/');
            self.expanded.insert(acc.clone());
        }
        self.reload();
        if let Some(i) = self.tree.iter().position(|r| r.rel == rel) {
            self.cur[index(Section::Notes)] = i;
        }
    }

    // ---------- notes open, in tabs ----------

    /// Opens a note in the editor (reading it, or writing): in its own tab,
    /// or the tab it already has.
    pub fn open_note(&mut self, rel: &str, reading: bool) {
        let Some(p) = self.project.clone() else { return };
        let key = (p.name.clone(), rel.to_string());
        self.remember_recent(rel);
        if let Some(o) = self.open.as_mut()
            && o.project == key.0
            && o.rel == key.1
        {
            o.reading = reading;
            return;
        }
        if let Some(i) = self.tabs.iter().position(|t| t.project == key.0 && t.rel == key.1) {
            self.park();
            let mut t = self.tabs.remove(i);
            t.reading = reading;
            t.checked = Instant::now() - std::time::Duration::from_secs(10);
            self.open = Some(t);
            self.watch_outside();
            return;
        }
        let text = crate::project::files::read_text(&p.dir.join(rel)).unwrap_or_default();
        let note = crate::note::Note::parse(&text);
        self.park();
        self.open = Some(Open {
            project: p.name.clone(),
            rel: rel.to_string(),
            ed: Editor::new(&note.body),
            base: text,
            reading,
            find: None,
            last_find: String::new(),
            links: Vec::new(),
            link_cur: 0,
            outside: false,
            checked: Instant::now(),
            resolving: None,
        });
        if !self.tab_order.contains(&key) {
            self.tab_order.push(key);
        }
    }

    /// The note shown goes behind: its tab stays.
    pub fn park(&mut self) {
        if let Some(o) = self.open.take() {
            self.tabs.push(o);
        }
    }

    /// The tab at `i` (in `tab_order`) shown.
    pub fn activate(&mut self, i: usize) {
        let Some(key) = self.tab_order.get(i).cloned() else { return };
        if self.open.as_ref().is_some_and(|o| o.project == key.0 && o.rel == key.1) {
            return;
        }
        let Some(j) = self.tabs.iter().position(|t| t.project == key.0 && t.rel == key.1) else { return };
        self.park();
        let mut t = self.tabs.remove(j);
        t.checked = Instant::now() - std::time::Duration::from_secs(10);
        self.open = Some(t);
        self.watch_outside();
    }

    /// The tab shown now, in `tab_order`.
    pub fn active_tab(&self) -> Option<usize> {
        let o = self.open.as_ref()?;
        self.tab_order.iter().position(|(p, r)| *p == o.project && *r == o.rel)
    }

    fn next_tab(&mut self, by: isize) {
        let n = self.tab_order.len();
        if n == 0 {
            return;
        }
        let i = match self.active_tab() {
            Some(i) => (i as isize + by).rem_euclid(n as isize) as usize,
            None => 0,
        };
        self.activate(i);
    }

    /// Closes the tab at `i`; asks first when it has changes not saved.
    pub fn close_tab(&mut self, i: usize) {
        if self.active_tab() == Some(i) {
            return self.close_note(false);
        }
        let Some(key) = self.tab_order.get(i).cloned() else { return };
        if let Some(j) = self.tabs.iter().position(|t| t.project == key.0 && t.rel == key.1) {
            if self.tabs[j].ed.dirty {
                // shown first, then asked
                self.activate(i);
                return self.close_note(false);
            }
            self.tabs.remove(j);
        }
        self.tab_order.remove(i);
    }

    /// Whether a note open (shown or behind) has changes not saved.
    pub fn unsaved(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.ed.dirty) || self.tabs.iter().any(|t| t.ed.dirty)
    }

    fn load_recent(&mut self) {
        self.recent = self
            .project
            .as_ref()
            .and_then(|p| crate::state::State::open(p).ok())
            .and_then(|s| s.get("ui.recent"))
            .map(|v| v.lines().map(str::to_string).collect())
            .unwrap_or_default();
    }

    fn remember_recent(&mut self, rel: &str) {
        self.recent.retain(|r| r != rel);
        self.recent.insert(0, rel.to_string());
        self.recent.truncate(12);
        self.home_list = self.home_rows();
        if let Some(p) = &self.project
            && let Ok(s) = crate::state::State::open(p)
        {
            let _ = s.set("ui.recent", &self.recent.join("\n"));
        }
    }

    // ---------- the editor ----------

    fn clipboard_set(&mut self, text: String) {
        if let Some(c) = self.clipboard.as_mut()
            && c.set_text(text.clone()).is_ok()
        {
            self.copied = text;
            return;
        }
        self.copied = text;
    }

    pub fn clipboard_get(&mut self) -> String {
        if let Some(c) = self.clipboard.as_mut()
            && let Ok(t) = c.get_text()
        {
            return t;
        }
        self.copied.clone()
    }

    fn editor_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let height = self.ed_area.height.max(1) as usize;
        let Some(o) = self.open.as_mut() else { return };
        // the search bar takes the keys while it is open
        if let Some(q) = o.find.as_mut() {
            match k.code {
                KeyCode::Esc => {
                    o.find = None;
                }
                KeyCode::Enter | KeyCode::F(3) => {
                    let q = q.clone();
                    o.last_find = q.clone();
                    if !o.ed.find(&q, shift) {
                        self.say(format!("\"{q}\" is not in this note"), true);
                    }
                }
                KeyCode::Backspace => {
                    q.pop();
                }
                KeyCode::Char(c) if !ctrl => q.push(c),
                _ => {}
            }
            return;
        }
        // a [[link]] being typed: its notes offered
        if !o.links.is_empty() {
            match k.code {
                KeyCode::Up => {
                    o.link_cur = o.link_cur.saturating_sub(1);
                    return;
                }
                KeyCode::Down => {
                    o.link_cur = (o.link_cur + 1).min(o.links.len() - 1);
                    return;
                }
                KeyCode::Enter | KeyCode::Tab => {
                    let name = o.links[o.link_cur].clone();
                    o.ed.complete_link(&name);
                    o.links.clear();
                    return;
                }
                KeyCode::Esc => {
                    o.links.clear();
                    return;
                }
                _ => {}
            }
        }
        if k.code == KeyCode::Char('w') && ctrl {
            return self.close_note(false);
        }
        if o.reading {
            match k.code {
                KeyCode::Esc | KeyCode::Char('q') => self.close_note(false),
                KeyCode::Char('e') | KeyCode::Enter => o.reading = false,
                KeyCode::Char('p') if ctrl => o.reading = false,
                KeyCode::Up => o.ed.scroll = o.ed.scroll.saturating_sub(1),
                KeyCode::Down => o.ed.scroll += 1,
                KeyCode::PageUp => o.ed.scroll = o.ed.scroll.saturating_sub(height),
                KeyCode::PageDown => o.ed.scroll += height,
                KeyCode::Home => o.ed.scroll = 0,
                _ => {}
            }
            return;
        }
        let ed = &mut o.ed;
        match k.code {
            KeyCode::Esc => {
                self.close_note(false);
                return;
            }
            KeyCode::Char('s') if ctrl => {
                self.save_note(false);
                return;
            }
            KeyCode::Char('z') if ctrl && shift => {
                ed.redo();
            }
            KeyCode::Char('z') if ctrl => {
                ed.undo();
            }
            KeyCode::Char('y') if ctrl => {
                ed.redo();
            }
            KeyCode::Char('a') if ctrl => ed.select_all(),
            KeyCode::Char('c') if ctrl => {
                let t = ed.copy();
                self.clipboard_set(t);
                self.say("copied", false);
                return;
            }
            KeyCode::Char('x') if ctrl => {
                if let Some(t) = ed.cut() {
                    self.clipboard_set(t);
                }
                return;
            }
            KeyCode::Char('v') if ctrl => {
                let t = self.clipboard_get();
                if let Some(o) = self.open.as_mut() {
                    o.ed.insert(&t);
                }
                return;
            }
            KeyCode::Char('f') if ctrl => {
                let start = ed.selected_text().filter(|s| !s.contains('\n')).unwrap_or_else(|| o.last_find.clone());
                o.find = Some(start);
                return;
            }
            KeyCode::F(3) => {
                let q = o.last_find.clone();
                o.ed.find(&q, shift);
                return;
            }
            KeyCode::Char('d') if ctrl => ed.insert_date(&crate::output::date(crate::output::now())),
            KeyCode::Char('t') if ctrl => ed.insert_table(),
            KeyCode::Char('p') if ctrl => o.reading = true,
            KeyCode::Char('o') if ctrl => {
                let note = o.rel.trim_end_matches(".md").to_string();
                self.ask(
                    Ask::AttachTo(note),
                    "Keep a file with this note: its path (or drop the file on the window)",
                    "",
                );
                return;
            }
            KeyCode::Enter if ctrl || alt => ed.toggle_todo(),
            KeyCode::Char('x') if alt => ed.toggle_todo(),
            KeyCode::Enter => ed.newline(),
            KeyCode::Backspace if ctrl => {
                ed.left(true, true);
                ed.backspace();
            }
            KeyCode::Backspace => ed.backspace(),
            KeyCode::Delete if ctrl => {
                ed.right(true, true);
                ed.delete();
            }
            KeyCode::Delete => ed.delete(),
            KeyCode::Left => ed.left(shift, ctrl),
            KeyCode::Right => ed.right(shift, ctrl),
            KeyCode::Up => ed.up(shift, 1),
            KeyCode::Down => ed.down(shift, 1),
            KeyCode::PageUp => ed.up(shift, height.saturating_sub(1).max(1)),
            KeyCode::PageDown => ed.down(shift, height.saturating_sub(1).max(1)),
            KeyCode::Home => ed.home(shift, ctrl),
            KeyCode::End => ed.end(shift, ctrl),
            KeyCode::Tab => {
                ed.tab(false);
            }
            KeyCode::BackTab => {
                ed.tab(true);
            }
            KeyCode::Char(c) if !ctrl => ed.insert(&c.to_string()),
            _ => {}
        }
        self.offer_links();
    }

    /// The notes whose names have the [[link]] being typed.
    fn offer_links(&mut self) {
        let Some(typed) = self.open.as_ref().and_then(|o| o.ed.link_prefix()) else {
            if let Some(o) = self.open.as_mut() {
                o.links.clear();
            }
            return;
        };
        let t = typed.to_lowercase();
        let names: Vec<String> = match self.project.clone() {
            Some(p) => data::all_notes(&p, &mut self.cache)
                .into_iter()
                .map(|(rel, _)| rel)
                .filter(|n| n.to_lowercase().contains(&t))
                .take(8)
                .collect(),
            None => Vec::new(),
        };
        if let Some(o) = self.open.as_mut() {
            o.link_cur = o.link_cur.min(names.len().saturating_sub(1));
            o.links = names;
        }
    }

    /// Whether the open note changed on disk since it was read.
    pub fn watch_outside(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        if o.checked.elapsed().as_millis() < 1500 {
            return;
        }
        o.checked = Instant::now();
        let Some(p) = self.home.project(&o.project) else { return };
        let now = crate::project::files::read_text(&p.dir.join(&o.rel)).unwrap_or_default();
        o.outside = now != o.base;
        // nothing changed here: just take theirs
        if o.outside && !o.ed.dirty {
            let note = crate::note::Note::parse(&now);
            let cur = o.ed.cur;
            let scroll = o.ed.scroll;
            o.ed = Editor::new(&note.body);
            o.ed.click(cur, false);
            o.ed.scroll = scroll;
            o.base = now;
            o.outside = false;
            self.say("this note was changed outside (an agent?): shown as it is now", false);
        }
    }

    /// Saves the open note; when it changed outside meanwhile, asks first.
    pub fn save_note(&mut self, force: bool) {
        let Some(o) = self.open.as_mut() else { return };
        let Some(p) = self.home.project(&o.project) else { return };
        let path = p.dir.join(&o.rel);
        let now = crate::project::files::read_text(&path).unwrap_or_default();
        if !force && now != o.base {
            self.modal = Some(Modal::Changed { theirs: now });
            return;
        }
        let body = o.ed.text();
        let rel = o.rel.clone();
        let resolving = o.resolving.clone();
        let result = crate::index::update_note(&p, &rel, |n, _, _| {
            n.body = if body.ends_with('\n') { body.clone() } else { format!("{body}\n") };
            Ok(())
        });
        match result {
            Ok((id, ())) => {
                let saved = crate::project::files::read_text(&path).unwrap_or_default();
                if let Some(o) = self.open.as_mut() {
                    o.base = saved;
                    o.ed.dirty = false;
                    o.outside = false;
                    o.resolving = None;
                }
                if let Ok(s) = crate::state::State::open(&p) {
                    let _ = s.record(crate::state::journal::Did::Write {
                        kind: rel.trim_end_matches(".md").to_string(),
                        id: id.clone(),
                        what: "changed by the user".into(),
                    });
                }
                crate::sync::trigger(&self.home);
                self.say(format!("saved {}", rel.trim_end_matches(".md")), false);
                // the other computer's copy is merged in now: taken out
                if let Some(copy) = resolving {
                    let ran = data::pg(&self.exe, &self.home.root, Some(&p.name), &["rm", copy.trim_end_matches(".md")]);
                    self.say(
                        if ran.ok {
                            "saved; the other computer's copy is merged in and archived".to_string()
                        } else {
                            data::first(&ran.out)
                        },
                        !ran.ok,
                    );
                }
                self.reload();
            }
            Err(e) => self.say(format!("not saved: {e}"), true),
        }
    }

    /// Merges what was changed here and outside since the note was opened
    /// (git merge-file: both changes kept, a clash marked).
    fn merge(&mut self, theirs: &str) {
        let Some(o) = self.open.as_mut() else { return };
        let base = crate::note::Note::parse(&o.base).body;
        let them = crate::note::Note::parse(theirs).body;
        let mine = o.ed.text();
        let dir = std::env::temp_dir().join(format!("paragma-merge-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let (b, m, t) = (dir.join("base"), dir.join("mine"), dir.join("theirs"));
        let ok =
            std::fs::write(&b, base).is_ok() && std::fs::write(&m, &mine).is_ok() && std::fs::write(&t, them).is_ok();
        let out = ok.then(|| {
            std::process::Command::new("git")
                .args(["merge-file", "-p", "-L", "yours", "-L", "before", "-L", "outside"])
                .arg(&m)
                .arg(&b)
                .arg(&t)
                .output()
        });
        let _ = std::fs::remove_dir_all(&dir);
        match out {
            Some(Ok(o2)) if o2.status.code().is_some() => {
                let merged = String::from_utf8_lossy(&o2.stdout).to_string();
                let clashes = o2.status.code().unwrap_or(0);
                o.ed.insert_whole(&merged);
                o.base = theirs.to_string();
                o.outside = false;
                if clashes > 0 {
                    self.say(format!("merged: {clashes} place(s) changed on both sides are marked <<<<<<< — keep what is right, then Ctrl+S"), true);
                } else {
                    self.say("merged: both changes kept; Ctrl+S saves", false);
                }
            }
            _ => self.say("could not merge (git not found): reload or keep yours", true),
        }
    }

    /// Closes the note shown (asks first when it has changes); the next tab
    /// is shown, or the sections when it was the last.
    pub fn close_note(&mut self, force: bool) {
        if !force && self.open.as_ref().is_some_and(|o| o.ed.dirty) {
            self.confirm(
                Confirm::CloseUnsaved,
                "Close without saving?",
                vec!["The changes since the last save are lost.".into()],
            );
            return;
        }
        let at = self.active_tab();
        self.open = None;
        if let Some(i) = at {
            self.tab_order.remove(i);
            if !self.tab_order.is_empty() {
                self.activate(i.min(self.tab_order.len() - 1));
            }
        }
        self.reload();
    }

    // ---------- windows ----------

    pub fn modal_key(&mut self, k: KeyEvent) {
        let Some(m) = self.modal.clone() else { return };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match m {
            Modal::Help | Modal::Message { .. } => self.modal = None,
            Modal::Input { ask, title, mut text } => match k.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Enter => {
                    self.modal = None;
                    self.answer(ask, text.trim().to_string());
                }
                KeyCode::Backspace => {
                    text.pop();
                    self.modal = Some(Modal::Input { ask, title, text });
                }
                KeyCode::Char('v') if ctrl => {
                    let t = self.clipboard_get();
                    text.push_str(t.lines().next().unwrap_or(""));
                    self.modal = Some(Modal::Input { ask, title, text });
                }
                KeyCode::Char(c) if !ctrl => {
                    text.push(c);
                    self.modal = Some(Modal::Input { ask, title, text });
                }
                _ => {}
            },
            Modal::Confirm { what, .. } => match k.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.modal = None;
                    self.confirmed(what);
                }
                KeyCode::Char('n') | KeyCode::Esc => self.modal = None,
                _ => {}
            },
            Modal::Choose { title, lines, options, cur } => match k.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Left | KeyCode::Up | KeyCode::BackTab => {
                    self.modal = Some(Modal::Choose { title, lines, options, cur: cur.saturating_sub(1) })
                }
                KeyCode::Right | KeyCode::Down | KeyCode::Tab => {
                    let n = options.len().saturating_sub(1);
                    self.modal = Some(Modal::Choose { title, lines, options, cur: (cur + 1).min(n) })
                }
                KeyCode::Enter => {
                    self.modal = None;
                    if let Some((_, a)) = options.get(cur) {
                        self.act(a.clone());
                    }
                }
                KeyCode::Char(c @ '1'..='9') => {
                    let i = (c as u8 - b'1') as usize;
                    if let Some((_, a)) = options.get(i) {
                        self.modal = None;
                        self.act(a.clone());
                    }
                }
                _ => {}
            },
            Modal::Changed { theirs } => match k.code {
                KeyCode::Char('m') => {
                    self.modal = None;
                    self.merge(&theirs);
                }
                KeyCode::Char('r') => {
                    self.modal = None;
                    if let Some(o) = self.open.as_mut() {
                        let note = crate::note::Note::parse(&theirs);
                        o.ed = Editor::new(&note.body);
                        o.base = theirs;
                        o.outside = false;
                    }
                    self.say("reloaded: the note as it is now", false);
                }
                KeyCode::Char('k') => {
                    self.modal = None;
                    self.save_note(true);
                }
                KeyCode::Esc => self.modal = None,
                _ => {}
            },
            Modal::Palette { .. } | Modal::Search { .. } => self.finder_key(k),
        }
    }

    /// A typed answer.
    fn answer(&mut self, ask: Ask, text: String) {
        let name = self.project_name();
        let run = |app: &mut App, args: Vec<String>| -> bool {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let ran = data::pg(&app.exe, &app.home.root, name.as_deref(), &refs);
            app.say(data::first(&ran.out), !ran.ok);
            app.reload();
            ran.ok
        };
        if text.is_empty() {
            return;
        }
        match ask {
            Ask::Goal => {
                if let Some(n) = name.clone() {
                    run(self, vec!["open".into(), n, "--goal".into(), text]);
                    self.say("goal saved", false);
                }
            }
            Ask::NewNote => {
                let rel = text.trim_matches('/').trim_end_matches(".md").to_string();
                let Some(p) = self.project.clone() else { return };
                let title = rel.rsplit('/').next().unwrap_or(&rel).to_string();
                match crate::index::update_note(&p, &format!("{rel}.md"), |n, exists, _| {
                    if !exists {
                        n.props.set_text("type", crate::note::Kind::Note.as_str());
                        n.props.set_text("title", title.clone());
                    }
                    Ok(())
                }) {
                    Ok((id, ())) => {
                        if let Ok(s) = crate::state::State::open(&p) {
                            let _ = s.record(crate::state::journal::Did::Write {
                                kind: rel.clone(),
                                id,
                                what: "created by the user".into(),
                            });
                        }
                        self.section = Section::Notes;
                        self.reveal(&format!("{rel}.md"));
                        self.open_note(&format!("{rel}.md"), false);
                    }
                    Err(e) => self.say(e.to_string(), true),
                }
            }
            Ask::NewFolder => {
                let f = format!("{}/", text.trim_matches('/'));
                if run(self, vec!["write".into(), f.clone()]) {
                    self.expanded.insert(f);
                    self.reload();
                }
            }
            Ask::Move(from) => {
                if text != from {
                    run(self, vec!["mv".into(), from, text]);
                }
            }
            Ask::NewProject => {
                let p = format!("{}/", text.trim_matches('/'));
                self.projects.clear();
                if run(self, vec!["write".into(), p]) {
                    self.projects = data::projects(&self.home);
                    if let Some(i) = self.projects.iter().position(|r| r.name == text.trim_matches('/')) {
                        self.choose_project(i);
                    }
                }
            }
            Ask::AddStep => {
                if let Some(n) = name.clone() {
                    run(self, vec!["open".into(), n, "--step".into(), text]);
                    self.say("step added", false);
                }
            }
            Ask::EditStep(n) => {
                run(self, vec!["steps".into(), "--edit".into(), n.to_string(), text]);
            }
            Ask::Answer(n) => {
                run(self, vec!["ask".into(), n.to_string(), text]);
            }
            Ask::Question => {
                run(self, vec!["ask".into(), text]);
            }
            Ask::Tell => {
                run(self, vec!["tell".into(), text]);
            }
            Ask::Attach => {
                // the path first, then the note it goes with
                self.pending_path = text;
                self.ask(Ask::AttachTo(String::new()), "With which note (folder/name)", "");
            }
            Ask::AttachTo(note) => {
                let (note, file) = if note.is_empty() {
                    // from the Files section: the path came first
                    (text.clone(), std::mem::take(&mut self.pending_path))
                } else {
                    (note, text.clone())
                };
                let paths =
                    super::panels::dropped_paths(&file).unwrap_or_else(|| vec![file.trim_matches('"').to_string()]);
                self.attach(&note, &paths);
            }
            Ask::NewRepo { moving } => self.new_repo(&text, moving),
            Ask::Place { kind, moving } => self.place_address(&kind, &text, moving),
            Ask::DavUser => {
                self.pending.user = text;
                self.ask(Ask::DavPassword, "Its password (an app password from its settings is best)", "");
            }
            Ask::DavPassword => {
                self.pending.password = text;
                self.place_env();
            }
            Ask::BackupFolder => {
                self.pending.folder = text;
                self.choose(
                    "Put the .env files (keys) into the backup too?",
                    vec!["Without them the backup holds no keys; with them, keep it somewhere safe.".into()],
                    vec![("No (recommended)", Act::BackupEnv(false)), ("Yes, the keys go too", Act::BackupEnv(true))],
                );
            }
            Ask::DailyFolder => {
                self.pending.folder = text;
                self.choose(
                    "Put the .env files (keys) into the daily backups too?",
                    vec!["Without them the backups hold no keys.".into()],
                    vec![("No (recommended)", Act::DailyEnv(false)), ("Yes, the keys go too", Act::DailyEnv(true))],
                );
            }
            Ask::RestorePath => self.act(Act::RestoreFrom(text.trim_matches('"').to_string())),
        }
    }

    /// Keeps files with a note (pg write --file --attach), one by one.
    pub fn attach(&mut self, note: &str, paths: &[String]) {
        let name = self.project_name();
        // a note open with changes: saved first, the card comes after
        if self.open.as_ref().is_some_and(|o| o.ed.dirty) {
            self.save_note(false);
        }
        let mut said = Vec::new();
        let mut ok = true;
        for f in paths {
            let ran = data::pg(&self.exe, &self.home.root, name.as_deref(), &["write", note, "--file", f, "--attach"]);
            ok &= ran.ok;
            said.push(data::first(&ran.out));
        }
        self.say(said.join("; "), !ok);
        if let Some((rel, reading)) = self.open.as_ref().map(|o| (o.rel.clone(), o.reading)) {
            // read again with its new card
            let text = self
                .project
                .as_ref()
                .and_then(|p| crate::project::files::read_text(&p.dir.join(&rel)).ok())
                .unwrap_or_default();
            if let Some(o) = self.open.as_mut()
                && !o.ed.dirty
            {
                o.ed = Editor::new(&crate::note::Note::parse(&text).body);
                o.base = text;
                o.reading = reading;
            }
        }
        self.reload();
    }

    fn confirmed(&mut self, what: Confirm) {
        let pg = |args: &[&str]| Act::Pg {
            args: args.iter().map(|s| s.to_string()).collect(),
            project: true,
            then: Then::Status,
        };
        match what {
            Confirm::Archive(rel) => {
                let what = rel.trim_end_matches(".md").trim_end_matches('/').to_string();
                self.act(pg(&["rm", &what]));
            }
            Confirm::DropStep(n) => self.act(pg(&["steps", "--drop", &n.to_string()])),
            Confirm::CloseUnsaved => self.close_note(true),
            Confirm::QuitUnsaved => {
                // the changes are let go: nothing open is kept
                self.open = None;
                self.tabs.clear();
                self.quit = true;
            }
            Confirm::FilesOn => self.act(Act::Pg {
                args: vec!["sync".into(), "files".into(), "on".into()],
                project: false,
                then: Then::Status,
            }),
            Confirm::Undo(n) => self.act(pg(&["undo", &n.to_string()])),
            Confirm::MoveInto(from, folder) => {
                let name = from.trim_end_matches('/').trim_end_matches(".md");
                let leaf = name.rsplit('/').next().unwrap_or(name);
                let to = format!("{}{leaf}", folder);
                self.act(pg(&["mv", name, &to]));
            }
            Confirm::Disconnect => self.act(Act::Pg {
                args: vec!["sync".into(), "disconnect".into()],
                project: false,
                then: Then::Message("Sync".into()),
            }),
        }
    }

    // ---------- the mouse ----------

    pub fn mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        let hit = self
            .hits
            .iter()
            .rev()
            .find(|(r, _)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
            .cloned();
        match m.kind {
            MouseEventKind::Moved => self.hover = Some((x, y)),
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let down = m.kind == MouseEventKind::ScrollDown;
                self.hover = Some((x, y));
                if self.modal.is_some() {
                    self.modal_key(KeyEvent::new(if down { KeyCode::Down } else { KeyCode::Up }, KeyModifiers::NONE));
                } else if let Some(o) = self.open.as_mut() {
                    o.ed.scroll = if down { o.ed.scroll + 3 } else { o.ed.scroll.saturating_sub(3) };
                } else if matches!(hit, Some((_, Hit::Preview))) {
                    self.preview_scroll =
                        if down { self.preview_scroll + 3 } else { self.preview_scroll.saturating_sub(3) };
                } else {
                    self.focus = Focus::Main;
                    self.move_cur(if down { 1 } else { -1 });
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let double =
                    self.last_click.is_some_and(|(t, lx, ly)| t.elapsed().as_millis() < 450 && lx == x && ly == y);
                self.last_click = Some((Instant::now(), x, y));
                let Some((rect, h)) = hit else { return };
                match h {
                    Hit::Section(i) if self.modal.is_none() => {
                        // the note shown goes behind its tab
                        self.park();
                        self.go(SECTIONS[i].0);
                    }
                    Hit::Tab(i) if self.modal.is_none() => self.activate(i),
                    Hit::TabClose(i) if self.modal.is_none() => self.close_tab(i),
                    Hit::Row(i) if self.modal.is_none() && self.open.is_none() => {
                        self.focus = Focus::Main;
                        if !self.acts(i) {
                            return;
                        }
                        self.set_cur(i);
                        if self.section == Section::Notes {
                            self.drag_from = Some(i);
                            self.drag_over = None;
                        }
                        if double {
                            self.drag_from = None;
                            self.section_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                        }
                    }
                    Hit::Row(i) => match self.modal.clone() {
                        Some(Modal::Palette { text, items, .. }) => {
                            self.modal = Some(Modal::Palette { text, items, cur: i });
                            self.modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                        }
                        Some(Modal::Search { text, results, note, .. }) => {
                            self.modal = Some(Modal::Search { text, results, note, cur: i });
                            self.modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                        }
                        _ => {}
                    },
                    Hit::Editor => {
                        let width = rect.width as usize;
                        if let Some(o) = self.open.as_mut()
                            && !o.reading
                        {
                            let rows = o.ed.rows(width);
                            let at = o.ed.place(&rows, o.ed.scroll + (y - rect.y) as usize, (x - rect.x) as usize);
                            let shift = m.modifiers.contains(KeyModifiers::SHIFT);
                            if double {
                                o.ed.select_word(at);
                            } else {
                                o.ed.click(at, shift);
                                self.dragging = true;
                            }
                        }
                    }
                    Hit::Button(b) => self.button(&b),
                    Hit::ModalButton(i) => match self.modal.clone() {
                        Some(Modal::Choose { title, lines, options, .. }) => {
                            self.modal = Some(Modal::Choose { title, lines, options, cur: i });
                            self.modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                        }
                        _ => {
                            let key = match (&self.modal, i) {
                                (Some(Modal::Confirm { .. }), 0) => 'y',
                                (Some(Modal::Confirm { .. }), _) => 'n',
                                (Some(Modal::Changed { .. }), 0) => 'm',
                                (Some(Modal::Changed { .. }), 1) => 'r',
                                (Some(Modal::Changed { .. }), 2) => 'k',
                                _ => '\u{1b}',
                            };
                            let code = if key == '\u{1b}' { KeyCode::Esc } else { KeyCode::Char(key) };
                            self.modal_key(KeyEvent::new(code, KeyModifiers::NONE));
                        }
                    },
                    _ => {}
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                let area = self.ed_area;
                if let Some(o) = self.open.as_mut() {
                    let rows = o.ed.rows(area.width as usize);
                    let vy = y.clamp(area.y, area.y + area.height.saturating_sub(1)) - area.y;
                    let vx = x.saturating_sub(area.x);
                    let at = o.ed.place(&rows, o.ed.scroll + vy as usize, vx as usize);
                    o.ed.click(at, true);
                }
            }
            // a note dragged in the tree: the row under it is where it goes
            MouseEventKind::Drag(MouseButton::Left) if self.drag_from.is_some() => {
                self.hover = Some((x, y));
                if let Some((_, Hit::Row(j))) = hit {
                    self.drag_over = Some(j);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.dragging = false;
                if let (Some(from), Some(over)) = (self.drag_from.take(), self.drag_over.take()) {
                    self.dropped_on(from, over);
                }
            }
            _ => {}
        }
    }

    /// A row of the tree dropped on another: into that folder (or the
    /// folder of that note).
    fn dropped_on(&mut self, from: usize, over: usize) {
        if from == over || self.section != Section::Notes {
            return;
        }
        let (Some(a), Some(b)) = (self.tree.get(from).cloned(), self.tree.get(over).cloned()) else { return };
        let folder = if b.folder { b.rel.clone() } else { parent(&b.rel) };
        let name = a.rel.trim_end_matches('/');
        // into itself, or where it already is
        if folder.starts_with(&a.rel) || parent(name) == folder {
            return;
        }
        let shown = if folder.is_empty() { "the top of the project".to_string() } else { folder.clone() };
        self.confirm(
            Confirm::MoveInto(a.rel.clone(), folder),
            &format!("Move {} into {shown}?", name.trim_end_matches(".md")),
            vec!["Links to it are changed to its new place (pg mv).".into()],
        );
    }

    /// A button of a toolbar or the header.
    fn button(&mut self, b: &str) {
        let key = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        match b {
            "save" => self.save_note(false),
            "close" => self.close_note(false),
            "find" => self.editor_key(key('f')),
            "date" => self.editor_key(key('d')),
            "table" => self.editor_key(key('t')),
            "todo" => self.editor_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
            "attach" => self.editor_key(key('o')),
            "read" | "edit" => {
                if let Some(o) = self.open.as_mut() {
                    o.reading = !o.reading;
                }
            }
            "new" => self.section_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
            "search" => self.search(""),
            "palette" => self.palette(),
            "help" => self.modal = Some(Modal::Help),
            "quit" => self.quit = true,
            "sync" => self.go(Section::Sync),
            _ => {}
        }
    }
}

/// The folder a note is in, with '/' ("" at the top).
pub fn parent(rel: &str) -> String {
    rel.trim_end_matches('/').rsplit_once('/').map(|(f, _)| format!("{f}/")).unwrap_or_default()
}

#[derive(Clone, Debug)]
pub enum PlanRow {
    Goal(String),
    Step(usize, crate::project::plan::Step),
    Question(usize, crate::project::plan::Question),
    Handoff(crate::project::plan::Handoff),
}

impl Editor {
    /// The whole text replaced (a merge), as one step to undo.
    pub fn insert_whole(&mut self, text: &str) {
        self.select_all();
        self.insert(text);
        self.click(Pos::default(), false);
    }
}
