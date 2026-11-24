//! The folder picker: where the memory goes, chosen by going through the
//! folders of the place (sync/browse.rs): a cloud's folder or any folder
//! of this computer, a WebDAV server's, a git repository's. Folders are
//! opened and made; the one chosen is said by what it holds: empty (the
//! memory goes there), a memory (an older copy of this one, or another:
//! what to do with it), or other files (the memory needs its own folder).

use super::app::{Act, App, Job, JobDone, Modal};
use crate::sync::browse::{self, Holds, Source};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::time::Instant;

/// What the folder chosen is for.
#[derive(Clone, Debug, PartialEq)]
pub enum For {
    /// A folder of this computer: a cloud's (its id) or any ("folder").
    Local(String),
    /// A folder of a WebDAV server, below the address typed.
    Dav(String),
    /// A folder of a git repository: another host's (its address), or a
    /// GitHub repository's (OWNER/NAME).
    Git(String),
    GitHub(String),
}

/// The picker's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Picker {
    pub what: For,
    pub source: Source,
    /// The folder it may not go above; None: up to the computer's disks.
    pub top: Option<String>,
    pub at: String,
    pub dirs: Vec<String>,
    pub holds: Holds,
    pub cur: usize,
    /// A listing that failed, said in the window.
    pub error: String,
    /// git: the repository's default branch (the one looked at).
    pub branch: String,
}

/// A line of the picker.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Here,
    New,
    Up,
    Dir(String),
}

impl Picker {
    pub fn rows(&self) -> Vec<Row> {
        let mut out = Vec::new();
        if !(self.at.is_empty() && self.source == Source::Local) {
            out.push(Row::Here);
            out.push(Row::New);
        }
        if self.can_go_up() {
            out.push(Row::Up);
        }
        out.extend(self.dirs.iter().cloned().map(Row::Dir));
        out
    }

    fn can_go_up(&self) -> bool {
        match &self.top {
            Some(t) => !same(&self.at, t),
            None => browse::parent(&self.source, &self.at).is_some(),
        }
    }

    /// Where the memory would be, for people.
    pub fn shown(&self) -> String {
        match &self.what {
            For::Local(_) if self.at.is_empty() => "this computer".into(),
            For::Local(_) => self.at.clone(),
            For::Dav(url) if self.at.is_empty() => url.clone(),
            For::Dav(url) => format!("{}/{}", url.trim_end_matches('/'), self.at),
            For::Git(url) => format!("{url}  /{}", self.at),
            For::GitHub(repo) => format!("{repo}  /{}", self.at),
        }
    }

    /// What the folder holds, for people.
    pub fn said(&self) -> String {
        if !self.error.is_empty() {
            return self.error.clone();
        }
        match &self.holds {
            _ if self.at.is_empty() && self.source == Source::Local => "choose a disk".into(),
            Holds::Nothing => "empty: the memory can go here".into(),
            Holds::Memory(_) => "holds a Paragma memory".into(),
            Holds::Other => "holds other files: the memory needs a folder of its own".into(),
        }
    }
}

/// Two paths the same, whatever their last separator.
fn same(a: &str, b: &str) -> bool {
    a.trim_end_matches(['/', '\\']) == b.trim_end_matches(['/', '\\'])
}

impl App {
    fn scratch(&self) -> std::path::PathBuf {
        crate::sync::dir(&self.home).join("browse")
    }

    /// The picker opened on a folder (its listing read now).
    pub fn open_picker(&mut self, what: For, source: Source, top: Option<String>, at: String, branch: String) {
        let mut p = Picker {
            what,
            source,
            top,
            at,
            dirs: Vec::new(),
            holds: Holds::Nothing,
            cur: 0,
            error: String::new(),
            branch,
        };
        self.relist(&mut p);
        self.modal = Some(Modal::Folders(Box::new(p)));
    }

    fn relist(&self, p: &mut Picker) {
        match browse::list(&p.source, &self.scratch(), &p.at) {
            Ok(l) => {
                p.dirs = l.dirs;
                p.holds = l.holds;
                p.error.clear();
            }
            Err(e) => {
                p.dirs.clear();
                p.holds = Holds::Other;
                p.error = crate::output::error_text(&e);
            }
        }
        p.cur = 0;
    }

    /// A cloud's folder, or any folder of this computer: where the picker
    /// opens (moving within one place: where the memory is now).
    pub fn pick_local(&mut self, kind: &str) {
        let now = crate::sync::Config::load(&self.home).filter(|_| self.pending.moving);
        let here_now = now.as_ref().filter(|c| c.kind == "folder").and_then(|c| {
            let at = std::path::PathBuf::from(&c.remote);
            let same_place = if kind == "folder" { true } else { c.cloud == kind };
            same_place.then(|| at.parent().map(|p| p.to_string_lossy().into_owned())).flatten()
        });
        let (top, start) = if kind == "folder" {
            (None, here_now.unwrap_or_else(|| std::env::home_dir().unwrap_or_default().to_string_lossy().into_owned()))
        } else {
            let Some(root) = crate::sync::cloud::state(kind).root else {
                return self.say(format!("{} has no folder here", crate::sync::cloud::name(kind)), true);
            };
            let root = root.to_string_lossy().into_owned();
            (Some(root.clone()), here_now.filter(|h| h.starts_with(&root)).unwrap_or(root))
        };
        self.open_picker(For::Local(kind.to_string()), Source::Local, top, start, String::new());
    }

    /// WebDAV, signed in: its folders below the address typed.
    pub fn pick_dav(&mut self, url: &str) {
        let p = self.pending.clone();
        let source = Source::Dav { url: url.to_string(), user: p.user, password: p.password };
        self.open_picker(For::Dav(url.to_string()), source, Some(String::new()), String::new(), String::new());
    }

    /// A git repository read in the background: its folders, or, empty,
    /// the memory straight at its root.
    pub fn pick_git(&mut self, what: For, url: String, gh: String) {
        let dir = self.scratch().join("repo");
        let (tx, rx) = std::sync::mpsc::channel();
        let shown = url.clone();
        std::thread::spawn(move || {
            let got = browse::open_git(&url, &dir, &gh).map_err(|e| crate::output::error_text(&e));
            let _ = tx.send(JobDone::Opened(got, what));
        });
        self.jobs.push(Job { what: format!("reading the folders of {shown}…"), rx, started: Instant::now() });
    }

    /// A GitHub repository chosen: the memory's own (paragma-memory) as it
    /// is, another one's folders gone through.
    pub fn pick_repo(&mut self, r: &crate::sync::github::Repo) {
        if r.full_name.rsplit('/').next() == Some(crate::sync::NEW_REPO) {
            return self.place_env();
        }
        let gh = crate::sync::github::gh_path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        self.pick_git(For::GitHub(r.full_name.clone()), r.clone_url.clone(), gh);
    }

    /// The repository read: its folders shown, or an empty one taken as it is.
    pub fn git_opened(&mut self, got: Result<Option<(Source, String)>, String>, what: For) {
        match got {
            Err(e) => self.say(e, true),
            Ok(None) => {
                self.pending.target = target(&what, "");
                self.place_env();
            }
            Ok(Some((source, branch))) => self.open_picker(what, source, Some(String::new()), String::new(), branch),
        }
    }

    pub fn picker_key(&mut self, mut p: Picker, k: KeyEvent) {
        let n = p.rows().len();
        match k.code {
            KeyCode::Esc => {
                self.modal = None;
                return;
            }
            KeyCode::Up => p.cur = p.cur.saturating_sub(1),
            KeyCode::Down => p.cur = (p.cur + 1).min(n.saturating_sub(1)),
            KeyCode::PageUp => p.cur = p.cur.saturating_sub(10),
            KeyCode::PageDown => p.cur = (p.cur + 10).min(n.saturating_sub(1)),
            KeyCode::Home => p.cur = 0,
            KeyCode::End => p.cur = n.saturating_sub(1),
            KeyCode::Backspace | KeyCode::Left => return self.picker_row(p, Row::Up),
            KeyCode::Char('n') => return self.picker_row(p, Row::New),
            KeyCode::Right => {
                if let Some(Row::Dir(d)) = p.rows().get(p.cur).cloned() {
                    return self.picker_row(p, Row::Dir(d));
                }
            }
            KeyCode::Enter => {
                if let Some(r) = p.rows().get(p.cur).cloned() {
                    return self.picker_row(p, r);
                }
            }
            _ => {}
        }
        self.modal = Some(Modal::Folders(Box::new(p)));
    }

    fn picker_row(&mut self, mut p: Picker, row: Row) {
        match row {
            Row::Up => {
                if p.can_go_up()
                    && let Some(up) = browse::parent(&p.source, &p.at)
                {
                    p.at = up;
                    self.relist(&mut p);
                }
                self.modal = Some(Modal::Folders(Box::new(p)));
            }
            Row::Dir(d) => {
                p.at = browse::join(&p.source, &p.at, &d);
                self.relist(&mut p);
                self.modal = Some(Modal::Folders(Box::new(p)));
            }
            Row::New => {
                self.pending.picker = Some(Box::new(p));
                self.ask(super::app::Ask::PickerFolder, "The new folder's name", "Paragma");
            }
            Row::Here => self.picker_choose(p),
        }
    }

    /// A folder named for the picker: made, and opened (`choose`: and
    /// chosen at once).
    pub fn picker_made(&mut self, mut p: Picker, name: &str, choose: bool) {
        match browse::make(&p.source, &self.scratch(), &p.at, name) {
            Ok(at) => {
                p.at = at;
                self.relist(&mut p);
                if choose && p.error.is_empty() {
                    return self.picker_choose(p);
                }
            }
            Err(e) => p.error = crate::output::error_text(&e),
        }
        self.modal = Some(Modal::Folders(Box::new(p)));
    }

    /// The folder chosen: by what it holds, the memory goes there, what
    /// is there is asked about, or another folder is needed.
    pub fn picker_choose(&mut self, p: Picker) {
        self.pending.target = target(&p.what, &p.at);
        self.pending.there.clear();
        let back = Act::Picker(Box::new(p.clone()));
        let main = format!("The main branch, {}", p.branch);
        let shared_repo = matches!(p.what, For::Git(_) | For::GitHub(_)) && !p.at.is_empty();
        match &p.holds {
            _ if !p.error.is_empty() => self.modal = Some(Modal::Folders(Box::new(p))),
            Holds::Nothing if shared_repo => self.choose(
                "The repository holds other things: on which branch?",
                vec![
                    "On a branch of its own, the repository's main branch is not touched.".into(),
                    "On the main branch, the memory's folder is seen with the rest.".into(),
                ],
                vec![("Its own branch, paragma (recommended)", Act::Branch(true)), (main.as_str(), Act::Branch(false))],
            ),
            Holds::Nothing => self.place_env(),
            Holds::Other => {
                let shown = p.shown();
                self.choose(
                    "This folder holds other files",
                    vec![
                        "The memory needs a folder of its own: what is in its folder becomes notes.".into(),
                        format!("In {shown}"),
                    ],
                    vec![
                        ("A new folder here: Paragma", Act::PickerMake(Box::new(p), "Paragma".into())),
                        ("Choose another folder", back),
                        ("Cancel", Act::Nothing),
                    ],
                )
            }
            Holds::Memory(id) => {
                // a memory found on the main branch is used there
                if shared_repo {
                    self.pending.target.push("--no-branch".into());
                }
                let ours = crate::sync::memory_id(&self.home);
                let note = "(To move the memory within one place, use Transfer.)".to_string();
                if !ours.is_empty() && *id == ours {
                    self.choose(
                        "An older copy of this memory is there",
                        vec![
                            "It was left there when the memory was connected elsewhere, or moved.".into(),
                            "Whatever is done, what changes is zipped into Downloads first.".into(),
                            note,
                        ],
                        vec![
                            ("Update it: made the same as this memory (recommended)", Act::Found("update".into())),
                            ("Join them: the notes of both kept", Act::Found("join".into())),
                            ("Remove it; nothing is connected", Act::RemoveThere),
                            ("Choose another folder", back),
                            ("Cancel", Act::Nothing),
                        ],
                    )
                } else {
                    self.choose(
                        "Another Paragma memory is there",
                        vec![
                            "It is not this computer's memory (another id).".into(),
                            "Whatever is done, what changes is zipped into Downloads first.".into(),
                            note,
                        ],
                        vec![
                            ("Join both: the notes of each kept", Act::Found("join".into())),
                            ("Replace it with this memory", Act::Found("replace".into())),
                            ("Make this computer's memory that one", Act::Found("take".into())),
                            ("Choose another folder", back),
                            ("Cancel", Act::Nothing),
                        ],
                    )
                }
            }
        }
    }
}

/// The pg sync arguments of the place chosen.
pub fn target(what: &For, at: &str) -> Vec<String> {
    match what {
        For::Local(_) => vec!["folder".into(), at.to_string()],
        For::Dav(url) => {
            let to = if at.is_empty() { url.clone() } else { format!("{}/{at}", url.trim_end_matches('/')) };
            vec!["webdav".into(), to]
        }
        For::Git(url) => {
            let mut v = vec!["git".into(), url.clone()];
            if !at.is_empty() {
                v.extend(["--folder".into(), at.to_string()]);
            }
            v
        }
        For::GitHub(repo) => {
            let mut v = vec!["--repo".into(), repo.clone()];
            if !at.is_empty() {
                v.extend(["--folder".into(), at.to_string()]);
            }
            v
        }
    }
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/ui/folders.rs"]
mod tests;
