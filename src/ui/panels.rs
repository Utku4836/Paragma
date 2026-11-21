//! The Home, Agents, Sync and Setup sections, the palette (Ctrl+K), the
//! search as words are typed, files dropped on the window, notes both
//! computers changed, and the work done in the background so the screen
//! never waits.

use super::app::{
    Accounted, Act, App, Ask, Confirm, Found, HomeRow, Job, JobDone, Modal, Outside, PAct, PItem, PlanRow, SECTIONS,
    Section, SyncRow, SyncView, Then,
};
use super::data::{self, AgentState, SetupRow};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

/// How long the typing rests before the search runs.
const TYPED: Duration = Duration::from_millis(140);
/// How often the files and pg's state are looked at for changes.
const WATCH: Duration = Duration::from_millis(1500);

impl App {
    // ---------- Home ----------

    /// The Home section, made again at each reload.
    pub fn home_rows(&mut self) -> Vec<HomeRow> {
        let Some(p) = self.project.clone() else {
            return vec![
                HomeRow::Title("Welcome to Paragma".into()),
                HomeRow::Line("The memory of your projects, for you and your coding agents.".into()),
                HomeRow::Blank,
                HomeRow::Projects,
            ];
        };
        let note = data::plan_note(&p);
        let mut out = vec![HomeRow::Goal(crate::project::plan::goal(&note)), HomeRow::Blank];
        out.push(HomeRow::Title("Next steps".into()));
        let claims: std::collections::HashMap<usize, String> =
            self.agents.iter().flat_map(|a| a.steps.iter().map(move |s| (*s, a.who.clone()))).collect();
        let open: Vec<(usize, crate::project::plan::Step)> = crate::project::plan::steps(&note)
            .into_iter()
            .enumerate()
            .filter(|(_, s)| s.open() && !s.deferred)
            .map(|(i, s)| (i + 1, s))
            .collect();
        if open.is_empty() {
            out.push(HomeRow::Line("Nothing open. a in Plan adds a step.".into()));
        }
        let more = open.len().saturating_sub(5);
        for (n, s) in open.into_iter().take(5) {
            let by = claims.get(&n).map(|w| format!("taken by {w}")).unwrap_or_default();
            out.push(HomeRow::Step(n, s.text, by));
        }
        if more > 0 {
            out.push(HomeRow::Line(format!("and {more} more in Plan")));
        }
        let due: Vec<(String, String, i64)> =
            data::due(&p, &mut self.cache).into_iter().filter(|d| d.2 <= 14).collect();
        if !due.is_empty() {
            out.push(HomeRow::Blank);
            out.push(HomeRow::Title("Due soon".into()));
            for (rel, d, left) in due.into_iter().take(5) {
                out.push(HomeRow::Due(rel, d, left));
            }
        }
        out.push(HomeRow::Blank);
        out.push(HomeRow::Title("Recent notes".into()));
        let recent: Vec<&String> = self.recent.iter().filter(|r| p.dir.join(r).is_file()).take(6).collect();
        if recent.is_empty() {
            out.push(HomeRow::Line("The notes you open are kept here.".into()));
        }
        for r in recent {
            out.push(HomeRow::Recent(r.clone()));
        }
        out.push(HomeRow::Blank);
        out.push(HomeRow::Title("Agents".into()));
        let busy: Vec<&data::AgentRow> = self.agents.iter().filter(|a| a.state == AgentState::AtWork).collect();
        if busy.is_empty() {
            out.push(HomeRow::Line("No conversation is at work here now.".into()));
        }
        for a in busy.into_iter().take(4) {
            out.push(HomeRow::Agent(a.who.clone(), a.doing.clone()));
        }
        out.push(HomeRow::Blank);
        out.push(HomeRow::Title("Sync".into()));
        out.push(match crate::sync::Config::load(&self.home) {
            None => HomeRow::Sync("Not connected: the memory is only on this computer.".into(), false),
            Some(c) => {
                let s = crate::sync::Status::load(&self.home);
                if !s.error.is_empty() {
                    HomeRow::Sync(format!("{}: the last sync failed: {}", c.place(), s.error), true)
                } else if !s.conflicts.is_empty() {
                    HomeRow::Sync(format!("{} changed on two computers: open Sync to choose", s.conflicts.len()), true)
                } else {
                    let when = if s.synced > 0 {
                        crate::output::ago(s.synced, crate::output::now())
                    } else {
                        "not yet".into()
                    };
                    let waiting = if self.unsent > 0 { format!(", {} to send", self.unsent) } else { String::new() };
                    HomeRow::Sync(format!("{}: synced {when}{waiting}", c.place()), false)
                }
            }
        });
        if let Some(h) = crate::project::plan::handoffs(&note).into_iter().next() {
            out.push(HomeRow::Blank);
            out.push(HomeRow::Title("Last handoff".into()));
            out.push(HomeRow::Handoff(h.heading, h.text));
        }
        out
    }

    pub fn home_key(&mut self, c: char, row: usize) {
        if c == 'n' && self.project.is_some() {
            return self.ask(Ask::NewNote, "New note (folder/name)", "");
        }
        if c != '\n' && c != '→' {
            return;
        }
        match self.home_list.get(row).cloned() {
            Some(HomeRow::Goal(g)) => self.ask(Ask::Goal, "The project's goal", &g),
            Some(HomeRow::Step(n, ..)) => {
                self.go(Section::Plan);
                if let Some(i) = self.plan_rows().iter().position(|r| matches!(r, PlanRow::Step(m, _) if *m == n)) {
                    self.set_cur(i);
                }
            }
            Some(HomeRow::Due(rel, ..)) => {
                let rel = format!("{rel}.md");
                self.reveal(&rel);
                self.open_note(&rel, false);
            }
            Some(HomeRow::Recent(rel)) => {
                self.reveal(&rel);
                self.open_note(&rel, false);
            }
            Some(HomeRow::Agent(..)) => self.go(Section::Agents),
            Some(HomeRow::Sync(..)) => self.go(Section::Sync),
            Some(HomeRow::Handoff(..)) => self.go(Section::Plan),
            Some(HomeRow::Projects) => self.go(Section::Projects),
            _ => {}
        }
    }

    // ---------- Agents ----------

    pub fn agents_key(&mut self, c: char, row: usize) {
        match (c, row) {
            ('t', _) | ('\n', 0) => {
                self.ask(Ask::Tell, "Tell every conversation here (they see it at their next pg command)", "")
            }
            ('\n', r) => {
                if let Some(a) = self.agents.get(r - 1) {
                    let now = crate::output::now();
                    let mut lines: Vec<String> = a
                        .did
                        .iter()
                        .take(30)
                        .map(|(at, d)| format!("{:>10}  {d}", crate::output::ago(*at, now)))
                        .collect();
                    if !a.steps.is_empty() {
                        let s: Vec<String> = a.steps.iter().map(|n| n.to_string()).collect();
                        lines.insert(0, format!("has taken step {}", s.join(", ")));
                        lines.insert(1, String::new());
                    }
                    self.modal = Some(Modal::Message { title: a.who.clone(), lines });
                }
            }
            _ => {}
        }
    }

    // ---------- Sync ----------

    pub fn sync_rows(&self) -> Vec<SyncRow> {
        if let SyncView::Repos { repos, filter, .. } = &self.sync_view {
            let f = filter.to_lowercase();
            let mut out = vec![SyncRow::NewRepo];
            out.extend(
                repos
                    .iter()
                    .filter(|r| f.is_empty() || r.full_name.to_lowercase().contains(&f))
                    .cloned()
                    .map(SyncRow::Repo),
            );
            return out;
        }
        let mut out = match crate::sync::Config::load(&self.home) {
            None => vec![SyncRow::Connect],
            Some(c) => vec![
                SyncRow::SyncNow,
                SyncRow::Env(c.env),
                SyncRow::Files(c.attachments),
                SyncRow::Limit(c.attach_mb),
                SyncRow::Transfer,
                SyncRow::Disconnect,
            ],
        };
        out.extend(self.conflicts.iter().cloned().map(SyncRow::Conflict));
        out
    }

    /// Typing in the list of repositories narrows it.
    pub fn sync_typing(&mut self, k: &KeyEvent) -> bool {
        let SyncView::Repos { filter, .. } = &mut self.sync_view else { return false };
        if k.modifiers.contains(KeyModifiers::CONTROL) {
            return false;
        }
        match k.code {
            KeyCode::Char(c) if !c.is_control() => filter.push(c),
            KeyCode::Backspace => {
                filter.pop();
            }
            _ => return false,
        }
        self.set_cur(if self.rows(Section::Sync) > 1 { 1 } else { 0 });
        true
    }

    pub fn sync_action(&mut self, row: usize) {
        let pg = |args: &[&str], then: Then| Act::Pg {
            args: args.iter().map(|s| s.to_string()).collect(),
            project: false,
            then,
        };
        let moving = matches!(self.sync_view, SyncView::Repos { moving: true, .. });
        let Some(r) = self.sync_rows().get(row).cloned() else { return };
        match r {
            SyncRow::Connect => self.connect_github(false),
            SyncRow::SyncNow => self.act(pg(&["git", "sync"], Then::Status)),
            SyncRow::Env(on) => self.act(pg(&["git", "env", if on { "off" } else { "on" }], Then::Status)),
            SyncRow::Files(true) => self.act(pg(&["git", "files", "off"], Then::Status)),
            SyncRow::Files(false) => {
                let mb = crate::sync::Config::load(&self.home).map(|c| c.attach_mb).unwrap_or(20);
                self.confirm(
                    Confirm::FilesOn,
                    "Send the files kept with notes too?",
                    vec![
                        format!("Workbooks, documents, images kept with notes, up to {mb} MB each."),
                        "A file sent to git stays in its history, also after it is taken out.".into(),
                    ],
                )
            }
            SyncRow::Limit(mb) => {
                let limits = [5u64, 10, 20, 50, 95];
                let next = limits.iter().position(|l| *l == mb).map(|i| limits[(i + 1) % limits.len()]).unwrap_or(20);
                self.act(pg(&["git", "files", "--limit", &next.to_string()], Then::Status));
            }
            SyncRow::Transfer => self.connect_github(true),
            SyncRow::Disconnect => {
                let place = crate::sync::Config::load(&self.home).map(|c| c.place()).unwrap_or_default();
                self.confirm(
                    Confirm::Disconnect,
                    "Stop syncing on this computer?",
                    vec![
                        format!("The projects stay here, and the memory stays in {place}."),
                        "Connecting again later joins them.".into(),
                    ],
                )
            }
            SyncRow::Conflict(rel) => self.resolve(&rel),
            SyncRow::NewRepo => self.ask(
                Ask::NewRepo { moving },
                "A new private repository: its name (OWNER/NAME for an organization)",
                "paragma-memory",
            ),
            SyncRow::Repo(r) => self.use_repo(&r, moving),
        }
    }

    /// Signs in (when needed) and lists the repositories, in the
    /// background; a memory found already is offered first.
    pub fn connect_github(&mut self, moving: bool) {
        self.go(Section::Sync);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let got = (|| -> anyhow::Result<Option<Accounted>> {
                let Some(a) = crate::sync::github::Account::saved()? else { return Ok(None) };
                let repos = a.repos()?;
                let found = if moving { None } else { a.find_memory(&repos)? };
                Ok(Some(Accounted { login: a.login.clone(), repos, found }))
            })();
            let _ = tx.send(JobDone::Account(got.map_err(|e| format!("{e:#}")), moving));
        });
        self.jobs.push(Job { what: "reading your GitHub repositories…".into(), rx });
    }

    /// Signed in (or not) outside the interface: the repositories next.
    pub fn signed_in(&mut self, ok: bool, moving: bool) {
        if ok {
            self.connect_github(moving);
        } else {
            self.say("not signed in to GitHub", true);
        }
    }

    fn accounted(&mut self, got: Result<Option<Accounted>, String>, moving: bool) {
        match got {
            Err(e) => self.say(format!("GitHub: {e}"), true),
            Ok(None) => self.choose(
                "Sign in to GitHub",
                vec![
                    "A page opens in your browser with a code to type there.".into(),
                    "This window is GitHub's until you are signed in, then it comes back.".into(),
                ],
                vec![("Sign in", Act::SignIn { moving }), ("Cancel", Act::Nothing)],
            ),
            Ok(Some(a)) => {
                self.sync_view =
                    SyncView::Repos { login: a.login.clone(), repos: a.repos, filter: String::new(), moving };
                self.go(Section::Sync);
                self.set_cur(0);
                if let Some((repo, prefix, branch)) = a.found {
                    let mut args = vec!["git".to_string(), "connect".into(), "--repo".into(), repo.full_name.clone()];
                    if !prefix.is_empty() {
                        args.push(if branch == crate::sync::BRANCH { "--branch".into() } else { "--no-branch".into() });
                    }
                    args.push("--yes".into());
                    self.choose(
                        "Your Paragma memory is on GitHub already",
                        vec![
                            format!(
                                "{} ({}) holds it.",
                                repo.full_name,
                                if repo.private { "private" } else { "public" }
                            ),
                            "Joining brings its projects here and sends this computer's; both are kept.".into(),
                        ],
                        vec![
                            ("Join it", Act::Pg { args, project: false, then: Then::Synced }),
                            ("Choose another repository", Act::Repos { moving: false }),
                            ("Cancel", Act::Nothing),
                        ],
                    );
                }
            }
        }
    }

    fn use_repo(&mut self, r: &crate::sync::github::Repo, moving: bool) {
        let name = r.full_name.clone();
        let pg = |args: Vec<&str>| Act::Pg {
            args: args.into_iter().map(str::to_string).collect(),
            project: false,
            then: Then::Synced,
        };
        let mut lines = vec![if r.private {
            format!("{name} is private: only the people with access to it read the memory.")
        } else {
            format!("{name} is PUBLIC: anyone can read the memory there.")
        }];
        if moving {
            lines.push("The memory is sent there, and this computer syncs with it from now on.".into());
            self.choose(
                &format!("Move the memory to {name}?"),
                lines,
                vec![
                    ("Move it, keep the old copy", pg(vec!["git", "move", "--repo", &name, "--yes"])),
                    (
                        "Move it, take it out of the old one",
                        pg(vec!["git", "move", "--repo", &name, "--remove-old", "--yes"]),
                    ),
                    ("Cancel", Act::Nothing),
                ],
            );
        } else {
            lines.push("A memory there already is joined with this one; else this one is sent.".into());
            self.choose(
                &format!("Connect to {name}?"),
                lines,
                vec![("Connect", pg(vec!["git", "connect", "--repo", &name, "--yes"])), ("Cancel", Act::Nothing)],
            );
        }
    }

    pub fn new_repo(&mut self, name: &str, moving: bool) {
        let name = if name.trim().is_empty() { "paragma-memory" } else { name.trim() };
        let pg = |args: Vec<&str>| Act::Pg {
            args: args.into_iter().map(str::to_string).collect(),
            project: false,
            then: Then::Synced,
        };
        if moving {
            self.choose(
                &format!("Move the memory to a new private repository, {name}?"),
                vec!["The memory is sent there, and this computer syncs with it from now on.".into()],
                vec![
                    ("Move it, keep the old copy", pg(vec!["git", "move", "--new", name, "--yes"])),
                    (
                        "Move it, take it out of the old one",
                        pg(vec!["git", "move", "--new", name, "--remove-old", "--yes"]),
                    ),
                    ("Cancel", Act::Nothing),
                ],
            );
        } else {
            self.act(pg(vec!["git", "connect", "--new", name, "--yes"]));
        }
    }

    // ---------- Setup ----------

    pub fn setup_key(&mut self, c: char, row: usize) {
        let pg = |args: &[&str], then: Then| Act::Pg {
            args: args.iter().map(|s| s.to_string()).collect(),
            project: false,
            then,
        };
        let Some(r) = self.setup.get(row).cloned() else { return };
        match (r, c) {
            (SetupRow::UpdateAll, '\n') => {
                self.act(pg(&["setup", "--update"], Then::Message("Updated to this version".into())))
            }
            (SetupRow::Agent { id, .. }, '\n' | 'i') => self.act(pg(&["setup", id, "--global"], Then::Setup)),
            (SetupRow::Agent { id, hooks: Some(on), .. }, 'h') => {
                self.act(pg(&["setup", id, "--global", if on { "--no-hooks" } else { "--hooks" }], Then::Setup))
            }
            (SetupRow::Agent { hooks: None, name, .. }, 'h') => {
                self.say(format!("{name} has no hooks; its instructions tell it what to run"), true)
            }
            (SetupRow::Mcp { id, on, .. }, '\n' | 'm') => {
                self.act(pg(&["setup", id, if on { "--no-mcp" } else { "--mcp" }], Then::Setup))
            }
            (SetupRow::Doctor, '\n') => self.act(Act::Pg {
                args: vec!["doctor".into()],
                project: true,
                then: Then::Message("pg doctor".into()),
            }),
            _ => {}
        }
    }

    // ---------- doing what was chosen ----------

    pub fn act(&mut self, a: Act) {
        match a {
            Act::Nothing => {}
            Act::Pg { args, project, then } => {
                let exe = self.exe.clone();
                let name = if project { self.project_name() } else { None };
                let what = format!("pg {}…", args.iter().take(2).cloned().collect::<Vec<_>>().join(" "));
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                    let ran = data::pg(&exe, name.as_deref(), &refs);
                    let _ = tx.send(JobDone::Ran(ran, then));
                });
                self.jobs.push(Job { what, rx });
            }
            Act::SignIn { moving } => self.run_outside = Some(Outside::SignIn(moving)),
            Act::Repos { moving } => {
                if matches!(self.sync_view, SyncView::Repos { .. }) {
                    self.go(Section::Sync);
                    self.set_cur(0);
                } else {
                    self.connect_github(moving);
                }
            }
            Act::Insert(t) => {
                if let Some(o) = self.open.as_mut() {
                    o.ed.insert(&t);
                }
            }
            Act::Attach { note, paths } => self.attach(&note, &paths),
            Act::ResolveMerge(note, copy) => {
                let Some(p) = self.project.clone() else { return };
                let mine =
                    crate::note::Note::parse(&crate::project::files::read_text(&p.dir.join(&note)).unwrap_or_default())
                        .body;
                let theirs =
                    crate::note::Note::parse(&crate::project::files::read_text(&p.dir.join(&copy)).unwrap_or_default())
                        .body;
                let (joined, clashes) = data::two_way(&mine, &theirs, "this computer", "the other computer");
                self.section = Section::Notes;
                self.reveal(&note);
                self.open_note(&note, false);
                if let Some(o) = self.open.as_mut() {
                    o.ed.insert_whole(&joined);
                    o.resolving = Some(copy);
                }
                self.say(
                    if clashes > 0 {
                        format!("{clashes} place(s) differ, marked <<<<<<< — keep what is right, then Ctrl+S")
                    } else {
                        "both versions joined; Ctrl+S saves and archives the other copy".to_string()
                    },
                    clashes > 0,
                );
            }
            Act::ResolveKeep(copy) => self.act(Act::Pg {
                args: vec!["rm".into(), copy.trim_end_matches(".md").into()],
                project: true,
                then: Then::Status,
            }),
            Act::ResolveTake(note, copy) => {
                let Some(p) = self.project.clone() else { return };
                let theirs =
                    crate::note::Note::parse(&crate::project::files::read_text(&p.dir.join(&copy)).unwrap_or_default())
                        .body;
                match crate::index::update_note(&p, &note, |n, _, _| {
                    n.body = theirs.clone();
                    Ok(())
                }) {
                    Ok((id, ())) => {
                        if let Ok(s) = crate::state::State::open(&p) {
                            let _ = s.record(crate::state::journal::Did::Write {
                                kind: note.trim_end_matches(".md").to_string(),
                                id,
                                what: "the other computer's version taken by the user".into(),
                            });
                        }
                        self.act(Act::ResolveKeep(copy));
                    }
                    Err(e) => self.say(e.to_string(), true),
                }
            }
        }
    }

    /// A note both computers changed: what to do with the two versions.
    pub fn resolve(&mut self, copy: &str) {
        let Some(note) = data::conflict_of(copy) else { return };
        let name = note.trim_end_matches(".md").to_string();
        self.choose(
            &format!("{name} changed on two computers"),
            vec![
                format!("This computer's version is {name}; the other one's is kept beside it."),
                "Merging shows both in the editor, the lines that differ marked.".into(),
            ],
            vec![
                ("Merge them in the editor", Act::ResolveMerge(note.clone(), copy.to_string())),
                ("Keep this computer's", Act::ResolveKeep(copy.to_string())),
                ("Take the other computer's", Act::ResolveTake(note, copy.to_string())),
                ("Cancel", Act::Nothing),
            ],
        );
    }

    // ---------- the background, the live watch ----------

    /// Called every frame: answers of the background work, the search
    /// typed, and what changed outside (agents, a sync).
    pub fn tick(&mut self) {
        self.poll_jobs();
        if self.search_due.is_some_and(|t| t.elapsed() >= TYPED) {
            self.search_due = None;
            self.run_search();
        }
        if self.watched.elapsed() >= WATCH {
            self.watched = Instant::now();
            if let Some(p) = &self.project {
                let s = data::signature(p);
                if s != self.sig {
                    self.reload();
                }
            }
            self.watch_outside();
        }
    }

    pub fn poll_jobs(&mut self) {
        let mut done = Vec::new();
        self.jobs.retain(|j| match j.rx.try_recv() {
            Ok(d) => {
                done.push(d);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        for d in done {
            match d {
                JobDone::Ran(ran, then) => {
                    match then {
                        Then::Status | Then::Setup => self.say(data::first(&ran.out), !ran.ok),
                        Then::Message(title) => self.show(&title, &ran.out),
                        Then::Synced => {
                            self.show("Sync", &ran.out);
                            if ran.ok {
                                self.sync_view = SyncView::Main;
                                self.set_cur(0);
                            }
                        }
                    }
                    if self.section == Section::Setup {
                        self.setup = data::setup_rows();
                    }
                    self.reload();
                }
                JobDone::Account(got, moving) => self.accounted(got, moving),
            }
        }
    }

    /// Waits for the work in the background to end (tests, quitting).
    pub fn finish_jobs(&mut self) {
        while !self.jobs.is_empty() {
            std::thread::sleep(Duration::from_millis(20));
            self.poll_jobs();
        }
    }

    fn show(&mut self, title: &str, out: &str) {
        let lines: Vec<String> = out.lines().take(40).map(str::to_string).collect();
        if self.modal.is_some() {
            self.say(data::first(out), false);
        } else {
            self.modal = Some(Modal::Message { title: title.into(), lines });
        }
    }

    // ---------- the palette and the search ----------

    pub fn palette(&mut self) {
        let items = self.palette_items("");
        self.modal = Some(Modal::Palette { text: String::new(), items, cur: 0 });
    }

    pub fn palette_items(&mut self, text: &str) -> Vec<PItem> {
        const CMDS: [(&str, &str, &str); 10] = [
            ("New note", "n", "new-note"),
            ("New folder", "N", "new-folder"),
            ("Search the memory", "/", "search"),
            ("Sync now", "", "sync"),
            ("Update the agents' instructions and hooks", "Setup", "update"),
            ("Check this project", "pg doctor", "doctor"),
            ("Tell the other conversations here", "pg tell", "tell"),
            ("New project", "", "new-project"),
            ("Keys", "?", "help"),
            ("Quit", "q", "quit"),
        ];
        let mut all: Vec<PItem> = Vec::new();
        let notes = match self.project.clone() {
            Some(p) => data::all_notes(&p, &mut self.cache),
            None => Vec::new(),
        };
        let note_item = |rel: &str, title: &str| PItem {
            label: rel.to_string(),
            hint: if rel.rsplit('/').next() == Some(title) { "note".into() } else { title.to_string() },
            act: PAct::Note(rel.to_string()),
        };
        if text.is_empty() {
            for r in self.recent.iter().take(6) {
                let rel = r.trim_end_matches(".md");
                if let Some((_, t)) = notes.iter().find(|(n, _)| n == rel) {
                    all.push(PItem { hint: format!("recent · {t}"), ..note_item(rel, t) });
                }
            }
        }
        let mut scored: Vec<(i64, PItem)> = Vec::new();
        for (label, hint, cmd) in CMDS {
            scored
                .push((fuzzy(text, label) + 5, PItem { label: label.into(), hint: hint.into(), act: PAct::Cmd(cmd) }));
        }
        for (i, (s, _, name)) in SECTIONS.iter().enumerate() {
            let label = format!("Go to {name}");
            scored.push((fuzzy(text, &label), PItem { label, hint: format!("{}", i + 1), act: PAct::Section(*s) }));
        }
        if !text.is_empty() {
            for (rel, title) in &notes {
                let s = fuzzy(text, rel).max(fuzzy(text, title));
                scored.push((s, note_item(rel, title)));
            }
        }
        scored.retain(|(s, _)| text.is_empty() || *s > 0);
        if !text.is_empty() {
            scored.sort_by_key(|s| std::cmp::Reverse(s.0));
        }
        all.extend(scored.into_iter().map(|(_, i)| i).take(40));
        if !text.is_empty() {
            all.push(PItem {
                label: format!("Search the memory for \u{201c}{text}\u{201d}"),
                hint: "Enter".into(),
                act: PAct::Search(text.to_string()),
            });
        }
        all
    }

    /// Opens the search, with words typed already or none.
    pub fn search(&mut self, text: &str) {
        self.modal = Some(Modal::Search {
            text: text.to_string(),
            results: Vec::new(),
            cur: 0,
            note: "type: the memory is searched as you type".into(),
        });
        if !text.is_empty() {
            self.run_search();
        }
    }

    /// Searches the memory for the words in the search window (the index
    /// opened once, brought up to date with the notes each time).
    pub fn run_search(&mut self) {
        let Some(Modal::Search { text, .. }) = &self.modal else { return };
        let text = text.trim().to_string();
        let Some(p) = self.project.clone() else {
            return self.set_found(Vec::new(), "choose a project first".into());
        };
        if text.is_empty() {
            return self.set_found(Vec::new(), "type: the memory is searched as you type".into());
        }
        if self.index.as_ref().is_none_or(|(n, _)| *n != p.name) {
            match crate::index::Index::open(&p) {
                Ok(i) => self.index = Some((p.name.clone(), i)),
                Err(e) => return self.set_found(Vec::new(), format!("the index: {e:#}")),
            }
        }
        let Some((_, index)) = self.index.as_mut() else { return };
        let _ = index.sync(&p);
        let hits = match crate::search::find(index, &text) {
            Ok(h) => h,
            Err(e) => return self.set_found(Vec::new(), format!("{e:#}")),
        };
        let top = hits.first().map(|h| h.score).unwrap_or(0.0);
        let mut found: Vec<Found> = Vec::new();
        for h in hits.into_iter().filter(|h| h.score >= top * 0.3) {
            let path = h.path.trim_end_matches(".md").to_string();
            if found.iter().any(|f| f.path == path) {
                continue;
            }
            found.push(Found {
                path,
                tag: format!("{} {}", h.id, h.kind.as_str()),
                heading: h.heading.clone(),
                snippet: crate::output::one_line(&h.text, 200),
            });
            if found.len() >= 30 {
                break;
            }
        }
        let note = if found.is_empty() {
            format!("nothing in memory on \u{201c}{text}\u{201d}")
        } else {
            crate::output::count(found.len() as i64, "note")
        };
        self.set_found(found, note);
    }

    fn set_found(&mut self, found: Vec<Found>, n: String) {
        if let Some(Modal::Search { results, cur, note, .. }) = self.modal.as_mut() {
            *results = found;
            *cur = 0;
            *note = n;
        }
    }

    /// Keys of the palette and the search window.
    pub fn finder_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let typed: Option<String> = match k.code {
            KeyCode::Char('v') if ctrl => Some(self.clipboard_get().lines().next().unwrap_or("").to_string()),
            KeyCode::Char(c) if !ctrl => Some(c.to_string()),
            _ => None,
        };
        match self.modal.as_mut() {
            Some(Modal::Palette { text, items, cur }) => match k.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Up => *cur = cur.saturating_sub(1),
                KeyCode::Down => *cur = (*cur + 1).min(items.len().saturating_sub(1)),
                KeyCode::PageDown => *cur = (*cur + 10).min(items.len().saturating_sub(1)),
                KeyCode::PageUp => *cur = cur.saturating_sub(10),
                KeyCode::Enter => {
                    let chosen = items.get(*cur).cloned();
                    self.modal = None;
                    if let Some(i) = chosen {
                        self.run_palette(i.act);
                    }
                }
                KeyCode::Backspace => {
                    text.pop();
                    let t = text.clone();
                    self.refilter(&t);
                }
                _ => {
                    if let Some(s) = typed {
                        text.push_str(&s);
                        let t = text.clone();
                        self.refilter(&t);
                    }
                }
            },
            Some(Modal::Search { text, results, cur, note }) => match k.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Up => *cur = cur.saturating_sub(1),
                KeyCode::Down => *cur = (*cur + 1).min(results.len().saturating_sub(1)),
                KeyCode::Enter => {
                    if let Some(f) = results.get(*cur).cloned() {
                        self.modal = None;
                        let rel = format!("{}.md", f.path);
                        self.section = Section::Notes;
                        self.reveal(&rel);
                        self.open_note(&rel, true);
                    } else {
                        self.run_search();
                    }
                }
                KeyCode::Backspace => {
                    text.pop();
                    *note = "…".into();
                    self.search_due = Some(Instant::now());
                }
                _ => {
                    if let Some(s) = typed {
                        text.push_str(&s);
                        *note = "…".into();
                        self.search_due = Some(Instant::now());
                    }
                }
            },
            _ => {}
        }
    }

    fn refilter(&mut self, text: &str) {
        let items = self.palette_items(text);
        if let Some(Modal::Palette { items: it, cur, .. }) = self.modal.as_mut() {
            *it = items;
            *cur = 0;
        }
    }

    fn run_palette(&mut self, a: PAct) {
        let pg = |args: &[&str], project: bool, then: Then| Act::Pg {
            args: args.iter().map(|s| s.to_string()).collect(),
            project,
            then,
        };
        match a {
            PAct::Section(s) => {
                self.park();
                self.go(s);
            }
            PAct::Note(rel) => {
                let rel = format!("{rel}.md");
                self.section = Section::Notes;
                self.reveal(&rel);
                self.open_note(&rel, false);
            }
            PAct::Search(t) => self.search(&t),
            PAct::Cmd(c) => match c {
                "new-note" => self.ask(Ask::NewNote, "New note (folder/name)", ""),
                "new-folder" => self.ask(Ask::NewFolder, "New folder", ""),
                "search" => self.search(""),
                "sync" => self.act(pg(&["git", "sync"], false, Then::Status)),
                "update" => {
                    self.act(pg(&["setup", "--update"], false, Then::Message("Updated to this version".into())))
                }
                "doctor" => self.act(pg(&["doctor"], true, Then::Message("pg doctor".into()))),
                "tell" => self.ask(Ask::Tell, "Tell every conversation here", ""),
                "new-project" => self.ask(Ask::NewProject, "New project: its name", ""),
                "help" => self.modal = Some(Modal::Help),
                "quit" => self.quit = true,
                _ => {}
            },
        }
    }

    // ---------- pasted text, files dropped on the window ----------

    pub fn paste(&mut self, text: &str) {
        let line = text.lines().next().unwrap_or("").to_string();
        match self.modal.as_mut() {
            Some(Modal::Input { text: t, .. }) => return t.push_str(&line),
            Some(Modal::Palette { text: t, .. }) => {
                t.push_str(&line);
                let t = t.clone();
                return self.refilter(&t);
            }
            Some(Modal::Search { text: t, .. }) => {
                t.push_str(&line);
                self.search_due = Some(Instant::now());
                return;
            }
            Some(_) => return,
            None => {}
        }
        let dropped = dropped_paths(text);
        if let Some(o) = self.open.as_mut() {
            if let Some(q) = o.find.as_mut() {
                return q.push_str(&line);
            }
            if o.reading {
                return;
            }
            match dropped {
                Some(paths) => {
                    let note = o.rel.trim_end_matches(".md").to_string();
                    self.choose(
                        "Keep the file with this note?",
                        paths.iter().map(|p| format!("  {p}")).collect(),
                        vec![
                            ("Keep it with the note", Act::Attach { note, paths }),
                            ("Paste the path as text", Act::Insert(text.to_string())),
                            ("Cancel", Act::Nothing),
                        ],
                    );
                }
                None => o.ed.insert(text),
            }
            return;
        }
        let Some(paths) = dropped else { return };
        // dropped on a note of the tree: kept with it
        let row = self.tree.get(self.cur()).cloned();
        match row {
            Some(r) if self.section == Section::Notes && !r.folder => {
                let note = r.rel.trim_end_matches(".md").to_string();
                self.choose(
                    &format!("Keep the file with {note}?"),
                    paths.iter().map(|p| format!("  {p}")).collect(),
                    vec![("Keep it with the note", Act::Attach { note, paths }), ("Cancel", Act::Nothing)],
                );
            }
            _ => {
                self.pending_path = paths.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join(" ");
                self.ask(Ask::AttachTo(String::new()), "Keep the file with which note (folder/name)", "");
            }
        }
    }
}

/// Paths of files, as a terminal pastes a file dropped on it: quoted when
/// they have spaces, several on one line. None when it is not only files.
pub fn dropped_paths(text: &str) -> Option<Vec<String>> {
    let t = text.trim();
    if t.is_empty() || t.len() > 8192 {
        return None;
    }
    let is_file = |p: &str| !p.is_empty() && std::path::Path::new(p).is_file();
    if !t.contains('"') && !t.contains('\'') && is_file(t) {
        return Some(vec![t.to_string()]);
    }
    let mut out = Vec::new();
    for line in t.lines() {
        let mut rest = line.trim();
        while !rest.is_empty() {
            let q = rest.chars().next().filter(|c| *c == '"' || *c == '\'');
            if let Some(q) = q {
                let r = &rest[1..];
                let e = r.find(q)?;
                out.push(r[..e].to_string());
                rest = r[e + 1..].trim_start();
            } else {
                let e = rest.find(char::is_whitespace).unwrap_or(rest.len());
                out.push(rest[..e].to_string());
                rest = rest[e..].trim_start();
            }
        }
    }
    (!out.is_empty() && out.iter().all(|p| is_file(p))).then_some(out)
}

/// How well typed letters name an item: the whole text in it (earlier and
/// at a word's start is better), else its letters in order; 0 when not.
pub fn fuzzy(typed: &str, item: &str) -> i64 {
    if typed.is_empty() {
        return 1;
    }
    let t = typed.to_lowercase();
    let i = item.to_lowercase();
    if let Some(at) = i.find(&t) {
        let start = at == 0 || !i[..at].chars().last().is_some_and(char::is_alphanumeric);
        return 1000 - at.min(900) as i64 + if start { 200 } else { 0 };
    }
    let mut it = i.chars();
    let mut gaps = 0i64;
    for c in t.chars() {
        let mut found = false;
        for x in it.by_ref() {
            if x == c {
                found = true;
                break;
            }
            gaps += 1;
        }
        if !found {
            return 0;
        }
    }
    (300 - gaps).max(1)
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/ui/panels.rs"]
mod tests;
