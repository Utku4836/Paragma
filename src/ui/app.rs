//! The interface's state and what keys and clicks do. Drawing is in
//! ui/draw.rs; what is shown comes from ui/data.rs.

use super::data::{self, ActRow, FileRow, ProjectRow, TreeRow};
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
    Projects,
    Notes,
    Plan,
    Files,
    Activity,
    Sync,
    Settings,
}

pub const SECTIONS: [(Section, &str, &str); 7] = [
    (Section::Projects, mark::PROJECTS, "Projects"),
    (Section::Notes, mark::NOTES, "Notes"),
    (Section::Plan, mark::PLAN, "Plan"),
    (Section::Files, mark::FILES, "Files"),
    (Section::Activity, mark::ACTIVITY, "Activity"),
    (Section::Sync, mark::SYNC, "Sync"),
    (Section::Settings, mark::SETTINGS, "Settings"),
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
    Search,
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
    /// The note changed outside the editor (an agent): what to do.
    Changed {
        theirs: String,
    },
    /// A list to choose from: search results (note paths), with titles.
    Pick {
        title: String,
        items: Vec<(String, String)>,
        cur: usize,
    },
    /// Lines to read, closed by any key.
    Message {
        title: String,
        lines: Vec<String>,
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
    /// The chosen row of each section's list.
    pub cur: [usize; 7],
    /// The first row shown of each section's list.
    pub top: [usize; 7],
    pub open: Option<Open>,
    pub modal: Option<Modal>,
    pub status: Option<(String, bool, Instant)>,
    pub clipboard: Option<arboard::Clipboard>,
    /// Kept when the system clipboard cannot be reached.
    pub copied: String,
    pub hits: Vec<(Rect, Hit)>,
    pub last_click: Option<(Instant, u16, u16)>,
    pub dragging: bool,
    pub quit: bool,
    /// Something outside the interface to run (pg git, pg setup), with the
    /// screen given to it meanwhile.
    pub run_outside: Option<Vec<String>>,
    /// The editor's text area, its visual rows and the width they wrap at.
    pub ed_area: Rect,
    /// A file's path typed in the Files section, waiting for its note.
    pub pending_path: String,
    /// Where the mouse is: the row under it is lit.
    pub hover: Option<(u16, u16)>,
    /// How far the preview is scrolled, and the note it is of.
    pub preview_scroll: usize,
    pub preview_of: String,
}

fn index(s: Section) -> usize {
    SECTIONS.iter().position(|x| x.0 == s).unwrap_or(0)
}

impl App {
    pub fn new(home: Home, project: Option<Project>) -> App {
        let mut app = App {
            home,
            exe: data::exe(),
            section: if project.is_some() { Section::Notes } else { Section::Projects },
            focus: Focus::Main,
            project,
            projects: Vec::new(),
            tree: Vec::new(),
            expanded: HashSet::new(),
            files: Vec::new(),
            activity: Vec::new(),
            cur: [0; 7],
            top: [0; 7],
            open: None,
            modal: None,
            status: None,
            clipboard: arboard::Clipboard::new().ok(),
            copied: String::new(),
            hits: Vec::new(),
            last_click: None,
            dragging: false,
            quit: false,
            run_outside: None,
            ed_area: Rect::default(),
            pending_path: String::new(),
            hover: None,
            preview_scroll: 0,
            preview_of: String::new(),
        };
        app.open_small_trees();
        app.reload();
        app
    }

    pub fn say(&mut self, text: impl Into<String>, bad: bool) {
        self.status = Some((text.into(), bad, Instant::now()));
    }

    fn project_name(&self) -> Option<String> {
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
        self.reload();
    }

    /// Reads again what the sections show.
    pub fn reload(&mut self) {
        self.projects = data::projects(&self.home);
        if let Some(p) = &self.project {
            self.project = self.home.project(&p.name);
        }
        match &self.project {
            Some(p) => {
                self.tree = data::tree(p, &self.expanded);
                self.files = data::kept_files(p);
                self.activity = data::activity(p);
            }
            None => {
                self.tree.clear();
                self.files.clear();
                self.activity.clear();
            }
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
            Section::Projects => self.projects.len(),
            Section::Notes => self.tree.len(),
            Section::Plan => self.plan_rows().len(),
            Section::Files => self.files.len(),
            Section::Activity => self.activity.len(),
            Section::Sync => self.sync_rows().len(),
            Section::Settings => SETTINGS.len(),
        }
    }

    pub fn cur(&self) -> usize {
        self.cur[index(self.section)]
    }

    fn set_cur(&mut self, v: usize) {
        let n = self.rows(self.section);
        self.cur[index(self.section)] = if n == 0 { 0 } else { v.min(n - 1) };
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

    pub fn sync_rows(&self) -> Vec<(&'static str, String)> {
        match crate::sync::Config::load(&self.home) {
            None => vec![("Connect to GitHub", "keep the memory on GitHub, the same on every computer".into())],
            Some(c) => vec![
                ("Sync now", String::new()),
                (".env files", if c.env { "sent".into() } else { "not sent".into() }),
                ("Files kept with notes", if c.attachments { "sent".into() } else { "not sent".into() }),
                ("File size limit", format!("{} MB a file", c.attach_mb)),
                ("Transfer", "move the memory to another repository".into()),
                ("Disconnect", String::new()),
            ],
        }
    }

    // ---------- keys ----------

    pub fn key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if self.modal.is_some() {
            self.modal_key(k);
            return;
        }
        if self.open.is_some() {
            self.editor_key(k);
            return;
        }
        match k.code {
            KeyCode::Char('q') if !ctrl => self.quit = true,
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('?') | KeyCode::F(1) => self.modal = Some(Modal::Help),
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = if self.focus == Focus::Sidebar { Focus::Main } else { Focus::Sidebar }
            }
            KeyCode::Char(c @ '1'..='7') => {
                self.section = SECTIONS[(c as u8 - b'1') as usize].0;
                self.focus = Focus::Main;
            }
            KeyCode::Char('/') => self.ask(Ask::Search, "Search memory", ""),
            KeyCode::F(5) => {
                self.reload();
                self.say("read again", false);
            }
            KeyCode::Up if self.focus == Focus::Sidebar => {
                let i = index(self.section);
                self.section = SECTIONS[(i + 6) % 7].0;
            }
            KeyCode::Down if self.focus == Focus::Sidebar => {
                let i = index(self.section);
                self.section = SECTIONS[(i + 1) % 7].0;
            }
            KeyCode::Enter | KeyCode::Right if self.focus == Focus::Sidebar => self.focus = Focus::Main,
            KeyCode::Left | KeyCode::Esc if self.focus == Focus::Main && !self.in_tree_folder_left(&k) => {
                self.focus = Focus::Sidebar
            }
            KeyCode::Up => self.set_cur(self.cur().saturating_sub(1)),
            KeyCode::Down => self.set_cur(self.cur() + 1),
            KeyCode::PageUp => self.set_cur(self.cur().saturating_sub(10)),
            KeyCode::PageDown => self.set_cur(self.cur() + 10),
            KeyCode::Home => self.set_cur(0),
            KeyCode::End => self.set_cur(usize::MAX),
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

    fn ask(&mut self, ask: Ask, title: &str, text: &str) {
        self.modal = Some(Modal::Input { ask, title: title.into(), text: text.into() });
    }

    fn confirm(&mut self, what: Confirm, title: &str, lines: Vec<String>) {
        self.modal = Some(Modal::Confirm { what, title: title.into(), lines });
    }

    /// Keys of the section shown.
    fn section_key(&mut self, k: KeyEvent) {
        let c = match k.code {
            KeyCode::Char(c) => c,
            KeyCode::Enter => '\n',
            KeyCode::Right => '→',
            KeyCode::Delete => 'd',
            _ => return,
        };
        let row = self.cur();
        match self.section {
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
                        "Keep a file with this note: its path",
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
                'a' => self.ask(Ask::Attach, "Keep a file: its path", ""),
                'g' => {
                    if let Some(f) = self.files.get(row).cloned() {
                        self.section = Section::Notes;
                        self.reveal(&format!("{}.md", f.note));
                    }
                }
                _ => {}
            },
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
            Section::Settings => {
                if c == '\n' {
                    self.settings_action(row);
                }
            }
        }
    }

    fn plan_key(&mut self, c: char, row: usize) {
        let rows = self.plan_rows();
        let name = self.project_name();
        let Some(r) = rows.get(row).cloned() else { return };
        let run = |app: &mut App, args: Vec<String>| {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let ran = data::pg(&app.exe, name.as_deref(), &refs);
            app.say(data::first(&ran.out), !ran.ok);
            app.reload();
        };
        match (r, c) {
            (_, 'a') => self.ask(Ask::AddStep, "New step", ""),
            (_, '?') => self.ask(Ask::Question, "A question for later", ""),
            (PlanRow::Step(n, s), ' ' | '\n' | 'x') => {
                let flag = if s.done { "--reopen" } else { "--done" };
                run(self, vec!["steps".into(), flag.into(), n.to_string()]);
            }
            (PlanRow::Step(n, s), 'l') => {
                let flag = if s.deferred { "--resume" } else { "--defer" };
                run(self, vec!["steps".into(), flag.into(), n.to_string()]);
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

    fn choose_project(&mut self, row: usize) {
        let Some(name) = self.projects.get(row).map(|p| p.name.clone()) else { return };
        self.project = self.home.project(&name);
        self.expanded.clear();
        self.open_small_trees();
        self.cur = [self.cur[0], 0, 0, 0, 0, self.cur[5], self.cur[6]];
        self.reload();
        self.section = Section::Notes;
        self.say(format!("project {name}"), false);
    }

    /// Opens the folders down to a note and puts the cursor on it.
    fn reveal(&mut self, rel: &str) {
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

    pub fn open_note(&mut self, rel: &str, reading: bool) {
        let Some(p) = &self.project else { return };
        let text = crate::project::files::read_text(&p.dir.join(rel)).unwrap_or_default();
        let note = crate::note::Note::parse(&text);
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
        });
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

    fn clipboard_get(&mut self) -> String {
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
                self.ask(Ask::AttachTo(note), "Keep a file with this note: its path", "");
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

    /// The notes whose names start with the [[link]] being typed.
    fn offer_links(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        let Some(typed) = o.ed.link_prefix() else {
            o.links.clear();
            return;
        };
        let t = typed.to_lowercase();
        let mut names: Vec<String> = self
            .tree
            .iter()
            .filter(|r| !r.folder)
            .map(|r| r.rel.trim_end_matches(".md").to_string())
            .filter(|n| n.to_lowercase().contains(&t))
            .take(8)
            .collect();
        // the folders closed in the tree have notes too
        if names.is_empty()
            && let Some(p) = &self.project
        {
            names = crate::project::scan::notes(&p.dir)
                .into_iter()
                .map(|n| n.rel.trim_end_matches(".md").to_string())
                .filter(|n| n.to_lowercase().contains(&t))
                .take(8)
                .collect();
        }
        o.link_cur = o.link_cur.min(names.len().saturating_sub(1));
        o.links = names;
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(Modal::Input { text: t, .. }) = self.modal.as_mut() {
            t.push_str(text.lines().next().unwrap_or(""));
            return;
        }
        if let Some(o) = self.open.as_mut() {
            if let Some(q) = o.find.as_mut() {
                q.push_str(text.lines().next().unwrap_or(""));
            } else if !o.reading {
                o.ed.insert(text);
            }
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
            o.ed = Editor::new(&note.body);
            o.ed.click(cur, false);
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

    fn close_note(&mut self, force: bool) {
        if !force && self.open.as_ref().is_some_and(|o| o.ed.dirty) {
            self.confirm(
                Confirm::CloseUnsaved,
                "Close without saving?",
                vec!["The changes since the last save are lost.".into()],
            );
            return;
        }
        self.open = None;
        self.reload();
    }

    // ---------- modals ----------

    fn modal_key(&mut self, k: KeyEvent) {
        let Some(m) = self.modal.clone() else { return };
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
                KeyCode::Char('v') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    let t = self.clipboard_get();
                    text.push_str(t.lines().next().unwrap_or(""));
                    self.modal = Some(Modal::Input { ask, title, text });
                }
                KeyCode::Char(c) => {
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
            Modal::Pick { title, items, cur } => match k.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Up => self.modal = Some(Modal::Pick { title, items, cur: cur.saturating_sub(1) }),
                KeyCode::Down => {
                    let n = items.len().saturating_sub(1);
                    self.modal = Some(Modal::Pick { title, items, cur: (cur + 1).min(n) })
                }
                KeyCode::Enter => {
                    self.modal = None;
                    if let Some((rel, _)) = items.get(cur) {
                        let rel = format!("{rel}.md");
                        self.section = Section::Notes;
                        self.reveal(&rel);
                        self.open_note(&rel, true);
                    }
                }
                _ => {}
            },
        }
    }

    /// A typed answer.
    fn answer(&mut self, ask: Ask, text: String) {
        let name = self.project_name();
        let run = |app: &mut App, args: Vec<String>| -> bool {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let ran = data::pg(&app.exe, name.as_deref(), &refs);
            app.say(data::first(&ran.out), !ran.ok);
            app.reload();
            ran.ok
        };
        if text.is_empty() && !matches!(ask, Ask::Search) {
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
                if run(self, vec!["write".into(), p])
                    && let Some(i) = self.projects.iter().position(|r| r.name == text.trim_matches('/'))
                {
                    self.choose_project(i);
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
                let file = file.trim_matches('"').to_string();
                // a note open with changes: saved first, the card comes after
                if self.open.as_ref().is_some_and(|o| o.ed.dirty) {
                    self.save_note(false);
                }
                let ok = run(self, vec!["write".into(), note.clone(), "--file".into(), file, "--attach".into()]);
                if ok && let Some(o) = self.open.as_ref().map(|o| (o.rel.clone(), o.reading)) {
                    self.open_note(&o.0, o.1);
                }
            }
            Ask::Search => {
                let ran = data::pg(&self.exe, name.as_deref(), &["find", &text, "--budget", "3000"]);
                let items: Vec<(String, String)> = ran
                    .out
                    .lines()
                    .filter_map(|l| {
                        let rest = l.strip_prefix('[')?;
                        let (tag, path) = rest.split_once("] ")?;
                        let path = path.split(['#', ' ']).next()?.to_string();
                        (!tag.starts_with("code") && !path.is_empty()).then(|| (path, tag.to_string()))
                    })
                    .collect();
                if items.is_empty() {
                    self.say(format!("nothing in memory on \"{text}\""), true);
                } else {
                    self.modal = Some(Modal::Pick { title: format!("Found: {text}"), items, cur: 0 });
                }
            }
        }
    }

    fn confirmed(&mut self, what: Confirm) {
        let name = self.project_name();
        let run = |app: &mut App, args: Vec<String>| {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let ran = data::pg(&app.exe, name.as_deref(), &refs);
            app.say(data::first(&ran.out), !ran.ok);
            app.reload();
        };
        match what {
            Confirm::Archive(rel) => {
                run(self, vec!["rm".into(), rel.trim_end_matches(".md").trim_end_matches('/').into()])
            }
            Confirm::DropStep(n) => run(self, vec!["steps".into(), "--drop".into(), n.to_string()]),
            Confirm::CloseUnsaved => self.close_note(true),
            Confirm::QuitUnsaved => self.quit = true,
            Confirm::FilesOn => run(self, vec!["git".into(), "files".into(), "on".into()]),
            Confirm::Undo(n) => run(self, vec!["undo".into(), n.to_string()]),
        }
    }

    fn sync_action(&mut self, row: usize) {
        let Some(c) = crate::sync::Config::load(&self.home) else {
            self.run_outside = Some(vec!["git".into()]);
            return;
        };
        let run = |app: &mut App, args: &[&str]| {
            let ran = data::pg(&app.exe, None, args);
            app.say(data::first(&ran.out), !ran.ok);
        };
        match row {
            0 => {
                self.say("syncing…", false);
                run(self, &["git", "sync"]);
                self.reload();
            }
            1 => run(self, &["git", "env", if c.env { "off" } else { "on" }]),
            2 if c.attachments => run(self, &["git", "files", "off"]),
            2 => self.confirm(
                Confirm::FilesOn,
                "Send the files kept with notes too?",
                vec![
                    format!("Workbooks, documents, images kept with notes, up to {} MB each.", c.attach_mb),
                    "A file sent to git stays in its history, also after it is taken out.".into(),
                ],
            ),
            3 => {
                let limits = [5u64, 10, 20, 50, 95];
                let next =
                    limits.iter().position(|l| *l == c.attach_mb).map(|i| limits[(i + 1) % limits.len()]).unwrap_or(20);
                run(self, &["git", "files", "--limit", &next.to_string()]);
            }
            // the repository chooser is pg git's own screen
            4 => self.run_outside = Some(vec!["git".into()]),
            5 => self.run_outside = Some(vec!["git".into()]),
            _ => {}
        }
    }

    fn settings_action(&mut self, row: usize) {
        match SETTINGS.get(row).map(|s| s.0) {
            Some("Agents") => self.run_outside = Some(vec!["setup".into()]),
            Some("Update the agents' instructions") => {
                let ran = data::pg(&self.exe, None, &["setup", "--update"]);
                self.say(data::first(&ran.out), !ran.ok);
            }
            Some("Check this project") => {
                let ran = data::pg(&self.exe, self.project_name().as_deref(), &["doctor"]);
                self.modal = Some(Modal::Message {
                    title: "pg doctor".into(),
                    lines: ran.out.lines().take(40).map(str::to_string).collect(),
                });
            }
            _ => {}
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
                if let Some(o) = self.open.as_mut() {
                    o.ed.scroll = if down { o.ed.scroll + 3 } else { o.ed.scroll.saturating_sub(3) };
                } else if matches!(hit, Some((_, Hit::Preview))) {
                    self.preview_scroll =
                        if down { self.preview_scroll + 3 } else { self.preview_scroll.saturating_sub(3) };
                } else if let Some(Modal::Pick { title, items, cur }) = self.modal.clone() {
                    let cur = if down { (cur + 1).min(items.len().saturating_sub(1)) } else { cur.saturating_sub(1) };
                    self.modal = Some(Modal::Pick { title, items, cur });
                } else if self.modal.is_none() {
                    let c = self.cur();
                    self.focus = Focus::Main;
                    self.set_cur(if down { c + 1 } else { c.saturating_sub(1) });
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let double =
                    self.last_click.is_some_and(|(t, lx, ly)| t.elapsed().as_millis() < 450 && lx == x && ly == y);
                self.last_click = Some((Instant::now(), x, y));
                let Some((rect, h)) = hit else { return };
                match h {
                    Hit::Section(i) if self.open.is_none() && self.modal.is_none() => {
                        self.section = SECTIONS[i].0;
                        self.focus = Focus::Main;
                    }
                    Hit::Row(i) if self.modal.is_none() && self.open.is_none() => {
                        self.focus = Focus::Main;
                        self.set_cur(i);
                        if double {
                            self.section_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                        }
                    }
                    Hit::Row(i) => {
                        if let Some(Modal::Pick { title, items, .. }) = self.modal.clone() {
                            self.modal = Some(Modal::Pick { title, items, cur: i });
                            if double {
                                self.modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                            }
                        }
                    }
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
                    Hit::ModalButton(i) => {
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
            MouseEventKind::Up(MouseButton::Left) => self.dragging = false,
            _ => {}
        }
    }

    /// A button of a toolbar or the footer.
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
            "search" => self.ask(Ask::Search, "Search memory", ""),
            "help" => self.modal = Some(Modal::Help),
            "quit" => self.quit = true,
            _ => {}
        }
    }
}

/// The folder a note is in, with '/' ("" at the top).
fn parent(rel: &str) -> String {
    rel.rsplit_once('/').map(|(f, _)| format!("{f}/")).unwrap_or_default()
}

#[derive(Clone, Debug)]
pub enum PlanRow {
    Goal(String),
    Step(usize, crate::project::plan::Step),
    Question(usize, crate::project::plan::Question),
    Handoff(crate::project::plan::Handoff),
}

pub const SETTINGS: [(&str, &str); 3] = [
    ("Agents", "which coding agents use Paragma, with instructions or hooks"),
    ("Update the agents' instructions", "bring Paragma's part of them to this version"),
    ("Check this project", "pg doctor: notes, code and indexes"),
];

impl Editor {
    /// The whole text replaced (a merge), as one step to undo.
    pub fn insert_whole(&mut self, text: &str) {
        self.select_all();
        self.insert(text);
        self.click(Pos::default(), false);
    }
}
