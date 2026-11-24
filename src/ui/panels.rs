//! The Home, Agents, Sync and Setup sections, the palette (Ctrl+K), the
//! search as words are typed, files dropped on the window, notes both
//! computers changed, and the work done in the background so the screen
//! never waits.

use super::app::{
    Accounted, Act, App, Ask, Confirm, Found, HomeRow, Job, JobDone, Modal, Outside, PAct, PItem, Pending, PlanRow,
    SECTIONS, Section, SyncRow, SyncView, Then,
};
use super::data::{self, AgentState, SetupRow};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

/// How long the typing rests before the search runs.
const TYPED: Duration = Duration::from_millis(140);
/// How often the files and pg's state are looked at for changes.
const WATCH: Duration = Duration::from_millis(1500);
/// How often whether the files are in the cloud is looked at again.
const CLOUD_LOOK: Duration = Duration::from_secs(15);

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

    /// The Sync section by its parts: the status, where the memory goes,
    /// what goes with it, backups, notes changed on two computers. Or the
    /// GitHub repositories, while one is being chosen.
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
        let config = crate::sync::Config::load(&self.home);
        let mut out = vec![SyncRow::Head("Status")];
        match &config {
            None => {
                out.push(SyncRow::Info("Not connected: the memory is only on this computer.".into(), false));
            }
            Some(c) => {
                let s = crate::sync::Status::load(&self.home);
                let now = crate::output::now();
                out.push(SyncRow::Info(
                    format!("{}: {}", crate::cli::sync_kind_name(&c.kind), c.place()),
                    c.kind == "github" && c.public,
                ));
                if let (false, Some((line, ok))) = (c.cloud.is_empty(), &self.cloud_line) {
                    out.push(SyncRow::Info(line.clone(), !ok));
                }
                out.push(SyncRow::Info(
                    if s.synced > 0 {
                        format!("last sync {}: {}", crate::output::ago(s.synced, now), s.did)
                    } else {
                        "not synced yet".into()
                    },
                    false,
                ));
                if self.unsent > 0 {
                    out.push(SyncRow::Info(
                        format!("{} changed here, to send", crate::output::count(self.unsent as i64, "note")),
                        false,
                    ));
                }
                if !s.error.is_empty() {
                    out.push(SyncRow::Info(s.error.clone(), true));
                }
                out.push(SyncRow::SyncNow);
            }
        }
        out.push(SyncRow::Head("Where"));
        match &config {
            None => out.push(SyncRow::Connect),
            Some(_) => {
                out.push(SyncRow::Transfer);
                out.push(SyncRow::Disconnect);
            }
        }
        if let Some(c) = &config {
            out.push(SyncRow::Head("What goes"));
            out.push(SyncRow::Env(c.env));
            out.push(SyncRow::Files(c.attachments));
            out.push(SyncRow::Limit(c.attach_mb));
        }
        out.push(SyncRow::Head("Backup"));
        out.push(SyncRow::BackupNow);
        let d = crate::sync::backup::Daily::load(&self.home);
        out.push(SyncRow::Daily(d.on, d.folder.clone()));
        out.push(SyncRow::Restore);
        if !self.conflicts.is_empty() {
            out.push(SyncRow::Head("Changed on two computers"));
            out.extend(self.conflicts.iter().cloned().map(SyncRow::Conflict));
        }
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
            SyncRow::Head(_) | SyncRow::Info(..) => {}
            SyncRow::Connect => self.choose_place(false),
            SyncRow::Transfer => self.choose_place(true),
            SyncRow::SyncNow => self.act(pg(&["sync", "now"], Then::Status)),
            SyncRow::Env(on) => self.act(pg(&["sync", "env", if on { "off" } else { "on" }], Then::Status)),
            SyncRow::Files(true) => self.act(pg(&["sync", "files", "off"], Then::Status)),
            SyncRow::Files(false) => {
                let mb = crate::sync::Config::load(&self.home).map(|c| c.attach_mb).unwrap_or(20);
                self.confirm(
                    Confirm::FilesOn,
                    "Send the files kept with notes too?",
                    vec![
                        format!("Workbooks, documents, images kept with notes, up to {mb} MB each."),
                        "In a git repository a file sent stays in its history, also after it is taken out.".into(),
                    ],
                )
            }
            SyncRow::Limit(mb) => {
                let limits = [5u64, 10, 20, 50, 95];
                let next = limits.iter().position(|l| *l == mb).map(|i| limits[(i + 1) % limits.len()]).unwrap_or(20);
                self.act(pg(&["sync", "files", "--limit", &next.to_string()], Then::Status));
            }
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
            SyncRow::BackupNow => {
                let downloads = std::env::home_dir().unwrap_or_default().join("Downloads");
                self.pending = Pending::default();
                self.ask(
                    Ask::BackupFolder,
                    "Back up the whole memory into one zip, in this folder",
                    &downloads.to_string_lossy(),
                );
            }
            SyncRow::Daily(true, _) => self.act(pg(&["sync", "backup", "--daily", "off"], Then::Status)),
            SyncRow::Daily(false, folder) => {
                let folder = if folder.is_empty() {
                    std::env::home_dir()
                        .unwrap_or_default()
                        .join("Documents")
                        .join("Paragma backups")
                        .to_string_lossy()
                        .into_owned()
                } else {
                    folder
                };
                self.pending = Pending::default();
                self.ask(Ask::DailyFolder, "A backup every day, the last 7 kept, in this folder", &folder);
            }
            SyncRow::Restore => self.choose_backup(),
            SyncRow::Conflict(rel) => self.resolve(&rel),
            SyncRow::NewRepo => self.ask(
                Ask::NewRepo { moving },
                "A new private repository: its name (OWNER/NAME for an organization)",
                "paragma-memory",
            ),
            SyncRow::Repo(r) => self.use_repo(&r, moving),
        }
    }

    /// Where to connect, or move the memory: GitHub, another git host, the
    /// cloud folders found here, any folder, WebDAV.
    pub fn choose_place(&mut self, moving: bool) {
        self.pending = Pending { moving, ..Pending::default() };
        let place = |kind: &str| Act::Place { kind: kind.into(), moving };
        let mut options: Vec<(String, Act)> = vec![("GitHub".into(), place("github"))];
        options.push(("Another git host (GitLab, Gitea, Bitbucket, your server)".into(), place("git")));
        for c in crate::sync::cloud::states().into_iter().filter(|c| c.installed) {
            let label = format!("{}  ({})", c.name, c.said());
            match c.hint() {
                None => options.push((label, place(c.id))),
                Some(h) => options.push((label, Act::Tell(c.name.to_string(), vec![h]))),
            }
        }
        options.push(("A folder (any other synced folder, a disk)".into(), place("folder")));
        options.push(("WebDAV (Nextcloud, ownCloud, a NAS)".into(), place("webdav")));
        options.push(("Cancel".into(), Act::Nothing));
        let title = if moving { "Move the memory to" } else { "Keep the memory in" };
        let lines = if moving {
            vec![
                "The memory is copied first, moved, then checked: the connection and every file.".into(),
                "Only when both checks pass is the old place left; else nothing changes.".into(),
            ]
        } else {
            vec!["Every computer connected to the same place has the same memory.".into()]
        };
        self.modal = Some(Modal::Choose { title: title.into(), lines, options, cur: 0 });
    }

    /// A kind of place chosen: what it needs next.
    fn place_kind(&mut self, kind: &str, moving: bool) {
        self.pending.moving = moving;
        match kind {
            "github" => self.connect_github(moving),
            "git" => self.ask(
                Ask::Place { kind: "git".into(), moving },
                "The repository's address (https://gitlab.com/me/notes.git; an empty private one is best)",
                "",
            ),
            "folder" => self.pick_local("folder"),
            "webdav" => self.ask(
                Ask::Place { kind: "webdav".into(), moving },
                "The WebDAV address of the memory's folder (Nextcloud: …/remote.php/dav/files/<you>/Paragma)",
                "https://",
            ),
            cloud => self.pick_local(cloud),
        }
    }

    /// An address typed for a place.
    pub fn place_address(&mut self, kind: &str, text: &str, moving: bool) {
        self.pending.moving = moving;
        let address = text.trim().trim_matches('"').trim_end_matches('/').to_string();
        self.pending.target = vec![kind.to_string(), address.clone()];
        match kind {
            "webdav" => self.ask(Ask::DavUser, "Your user name there", ""),
            "git" => self.pick_git(super::folders::For::Git(address.clone()), address, String::new()),
            _ => self.place_env(),
        }
    }

    /// Whether the .env files go too.
    pub fn place_env(&mut self) {
        self.choose(
            "Send the .env files (API keys) too?",
            vec![
                "Each project's keys stay on this computer unless you say so.".into(),
                "They never go to a public repository.".into(),
            ],
            vec![("No (recommended)", Act::PlaceEnv(false)), ("Yes, the keys go too", Act::PlaceEnv(true))],
        );
    }

    /// The last step: pg sync connect or move, in the background.
    fn place_go(&mut self, remove_old: bool) {
        let p = self.pending.clone();
        let mut args = vec!["sync".to_string(), if p.moving { "move" } else { "connect" }.to_string()];
        args.extend(p.target.iter().cloned());
        if p.env {
            args.push("--env".into());
        }
        if p.moving && remove_old {
            args.push("--remove-old".into());
        }
        if !p.there.is_empty() {
            args.extend(["--there".into(), p.there.clone()]);
        }
        args.push("--yes".into());
        let input = if p.target.first().map(String::as_str) == Some("webdav") {
            args.extend(["--user".into(), p.user.clone(), "--password-stdin".into()]);
            Some(p.password.clone())
        } else {
            None
        };
        self.pending.password.clear();
        let to = match p.target.first().map(String::as_str) {
            Some("--repo" | "--new") => p.target.get(1).cloned().unwrap_or_default(),
            Some("git") => "the git repository".into(),
            Some("folder") => p.target.get(1).cloned().unwrap_or_default(),
            Some("webdav") => "WebDAV".into(),
            Some(id) => crate::sync::cloud::name(id).to_string(),
            None => "GitHub".into(),
        };
        let what = if p.moving {
            format!("moving the memory to {to}: copied, sent, then every file checked…")
        } else {
            format!("connecting to {to}: every file sent and read back…")
        };
        self.run_pg(args, input, &what);
    }

    /// A pg command in the background, with a line on its input.
    fn run_pg(&mut self, args: Vec<String>, input: Option<String>, what: &str) {
        let exe = self.exe.clone();
        let root = self.home.root.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let ran = data::pg_input(&exe, &root, None, &refs, input.as_deref());
            let _ = tx.send(JobDone::Ran(ran, Then::Synced));
        });
        self.jobs.push(Job { what: what.into(), rx, started: Instant::now() });
    }

    /// The backups found on this computer, to restore one.
    fn choose_backup(&mut self) {
        let found = crate::sync::backup::found(&self.home);
        let mut options: Vec<(String, Act)> = found
            .iter()
            .take(8)
            .map(|(p, m)| {
                (
                    format!(
                        "{}  from {}  ·  {}, {}",
                        crate::output::date_time(m.created),
                        if m.host.is_empty() { "?" } else { &m.host },
                        crate::output::count(m.projects() as i64, "project"),
                        crate::output::count(m.files.len() as i64, "file")
                    ),
                    Act::RestoreFrom(p.to_string_lossy().into_owned()),
                )
            })
            .collect();
        options.push(("Another backup (its path)".into(), Act::RestoreFrom(String::new())));
        options.push(("Cancel".into(), Act::Nothing));
        let lines = if found.is_empty() {
            vec!["No backup found in Downloads, Desktop, Documents or the cloud folders.".into()]
        } else {
            vec![format!(
                "{} found on this computer, the newest first.",
                crate::output::count(found.len() as i64, "backup")
            )]
        };
        self.modal = Some(Modal::Choose { title: "Restore a backup".into(), lines, options, cur: 0 });
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
            let _ = tx.send(JobDone::Account(got.map_err(|e| crate::output::error_text(&e)), moving));
        });
        self.jobs.push(Job { what: "reading your GitHub repositories…".into(), rx, started: Instant::now() });
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
                    let mut target = vec!["--repo".to_string(), repo.full_name.clone()];
                    if !prefix.is_empty() {
                        target.push(if branch == crate::sync::BRANCH {
                            "--branch".into()
                        } else {
                            "--no-branch".into()
                        });
                    }
                    self.pending = Pending { moving: false, target, ..Pending::default() };
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
                            ("Join it", Act::PlaceOld(false)),
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
        self.pending = Pending { moving, target: vec!["--repo".into(), name.clone()], ..Pending::default() };
        if r.private {
            return self.pick_repo(r);
        }
        self.choose(
            &format!("{name} is PUBLIC"),
            vec!["Anyone can read the memory there. A private repository is better.".into()],
            vec![("Use it anyway", Act::PickRepo(r.clone())), ("Cancel", Act::Nothing)],
        );
    }

    pub fn new_repo(&mut self, name: &str, moving: bool) {
        let name = if name.trim().is_empty() { "paragma-memory" } else { name.trim() };
        self.pending = Pending { moving, target: vec!["--new".into(), name.to_string()], ..Pending::default() };
        self.place_env();
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
                let root = self.home.root.clone();
                let name = if project { self.project_name() } else { None };
                let what = format!("pg {}…", args.iter().take(2).cloned().collect::<Vec<_>>().join(" "));
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                    let ran = data::pg(&exe, &root, name.as_deref(), &refs);
                    let _ = tx.send(JobDone::Ran(ran, then));
                });
                self.jobs.push(Job { what, rx, started: Instant::now() });
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
            Act::Place { kind, moving } => self.place_kind(&kind, moving),
            Act::Tell(title, lines) => self.choose(&title, lines, vec![("OK", Act::Nothing)]),
            Act::Picker(p) => self.modal = Some(Modal::Folders(p)),
            Act::PickRepo(r) => self.pick_repo(&r),
            Act::PickerMake(p, name) => self.picker_made(*p, &name, true),
            Act::Branch(own) => {
                if !own {
                    self.pending.target.push("--no-branch".into());
                }
                self.place_env();
            }
            Act::Found(there) => {
                self.pending.there = there;
                self.place_env();
            }
            Act::RemoveThere => {
                let p = self.pending.clone();
                let mut args = vec!["sync".to_string(), "remove-there".into()];
                args.extend(p.target.iter().cloned());
                let input = if p.target.first().map(String::as_str) == Some("webdav") {
                    args.extend(["--user".into(), p.user.clone(), "--password-stdin".into()]);
                    Some(p.password.clone())
                } else {
                    None
                };
                self.pending.password.clear();
                self.run_pg(args, input, "taking the older copy out (zipped first)…");
            }
            Act::PlaceEnv(env) => {
                self.pending.env = env;
                if self.pending.moving {
                    self.choose(
                        "And the old place?",
                        vec![
                            "Either way it is left only after the new one is checked.".into(),
                            "Kept, it says where the memory went, so the other computers follow.".into(),
                        ],
                        vec![
                            ("Keep a copy there (recommended)", Act::PlaceOld(false)),
                            ("Take the memory out of it", Act::PlaceOld(true)),
                        ],
                    );
                } else {
                    self.place_go(false);
                }
            }
            Act::PlaceOld(remove) => self.place_go(remove),
            Act::BackupEnv(env) => {
                let mut args = vec!["sync".to_string(), "backup".into(), self.pending.folder.clone()];
                if env {
                    args.push("--env".into());
                }
                self.act(Act::Pg { args, project: false, then: Then::Message("Backup".into()) });
            }
            Act::DailyEnv(env) => {
                let mut args = vec![
                    "sync".to_string(),
                    "backup".into(),
                    "--daily".into(),
                    "on".into(),
                    "--to".into(),
                    self.pending.folder.clone(),
                ];
                if env {
                    args.push("--env".into());
                }
                self.act(Act::Pg { args, project: false, then: Then::Status });
            }
            Act::RestoreFrom(p) if p.is_empty() => self.ask(Ask::RestorePath, "The backup's zip (its full path)", ""),
            Act::RestoreFrom(p) => {
                let what = crate::sync::backup::manifest(std::path::Path::new(&p));
                match what {
                    Err(e) => self.say(crate::output::error_text(&e), true),
                    Ok(m) => self.choose(
                        "Restore this backup?",
                        vec![
                            format!(
                                "From {}, {}: {}, {}.",
                                if m.host.is_empty() { "?" } else { &m.host },
                                crate::output::date_time(m.created),
                                crate::output::count(m.projects() as i64, "project"),
                                crate::output::count(m.files.len() as i64, "file")
                            ),
                            "The memory is copied first; if anything fails it is put back as it was.".into(),
                        ],
                        vec![
                            (
                                "Join it with the memory (a note that differs kept beside)",
                                Act::RestoreMode(p.clone(), false),
                            ),
                            ("Put it in the memory's place (what it does not have goes)", Act::RestoreMode(p, true)),
                            ("Cancel", Act::Nothing),
                        ],
                    ),
                }
            }
            Act::RestoreMode(p, replace) => {
                let mut args = vec!["sync".to_string(), "restore".into(), p];
                if replace {
                    args.extend(["--replace".into(), "--yes".into()]);
                }
                self.act(Act::Pg { args, project: false, then: Then::Message("Restore".into()) });
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
        self.watch_cloud();
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

    /// While Sync is shown for a cloud folder: whether the files are in
    /// the cloud, looked at again every little while, in the background.
    fn watch_cloud(&mut self) {
        if let Some(rx) = &self.cloud_rx {
            match rx.try_recv() {
                Ok(line) => {
                    self.cloud_line = line;
                    self.cloud_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(_) => self.cloud_rx = None,
            }
            return;
        }
        if self.section != Section::Sync || self.cloud_at.is_some_and(|t| t.elapsed() < CLOUD_LOOK) {
            return;
        }
        self.cloud_at = Some(Instant::now());
        let Some(c) = crate::sync::Config::load(&self.home).filter(|c| !c.cloud.is_empty()) else {
            self.cloud_line = None;
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::sync::cloud::upload_line(&c.cloud, &c.path, &c.id));
        });
        self.cloud_rx = Some(rx);
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
                JobDone::Opened(got, what) => self.git_opened(got, what),
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
                Err(e) => return self.set_found(Vec::new(), format!("the index: {}", crate::output::error_text(&e))),
            }
        }
        let Some((_, index)) = self.index.as_mut() else { return };
        let _ = index.sync(&p);
        let hits = match crate::search::find(index, &text) {
            Ok(h) => h,
            Err(e) => return self.set_found(Vec::new(), crate::output::error_text(&e)),
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
                "sync" => self.act(pg(&["sync", "now"], false, Then::Status)),
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
