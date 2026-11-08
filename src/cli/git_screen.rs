//! The screen of `pg git` in a terminal. Not connected: sign in, find the
//! memory again (restore it here?), or choose where it goes (a new private
//! repository, or one you have: its own branch? the .env files too?).
//! Connected: the account, the repository, the last sync, and what can be
//! done: sync now, the .env setting, move, disconnect.

use super::Context;
use super::screen::{banner, draw, fit, row};
use super::setup::Style;
use crate::sync::github::{Account, Repo};
use crate::sync::{self, BRANCH, Config, FOLDER, NEW_REPO, Status};
use crate::terminal::{self, Key};
use anyhow::Result;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Ask {
    SignIn,
    Restore,
    Public,
    Branch,
    Env,
    RemoveOld,
    Disconnect,
}

#[derive(Clone, PartialEq, Debug)]
enum Screen {
    /// Connected: what can be done.
    Home,
    /// Where the memory goes: a new repository or one of these.
    Repos,
    Ask(Ask),
    /// What was done, until a key.
    Done,
}

/// A row of the repository list.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Row {
    /// A new private repository of this owner (in `owners`).
    New(usize),
    Repo(usize),
}

/// The rows of the connected screen.
const ACTIONS: [&str; 5] = ["Sync now", ".env files", "Move to another repository", "Disconnect", "Quit"];

struct Page<'a> {
    cx: &'a Context,
    style: &'a Style,
    screen: Screen,
    cur: usize,
    top: usize,
    account: Option<Account>,
    repos: Vec<Repo>,
    /// Where a new repository can be made: the account, its organizations.
    owners: Vec<String>,
    search: String,
    /// The memory found in a repository, offered to restore.
    found: Option<(Repo, String, String)>,
    /// Being chosen: the repository (None: a new one of `owner`), its
    /// folder and branch.
    chosen: Option<Repo>,
    owner: String,
    prefix: String,
    branch: String,
    /// What was answered about the .env files; None: not asked.
    env: Option<bool>,
    /// Choosing a repository to move to, not to connect.
    moving: bool,
    message: Vec<String>,
    quit: bool,
}

pub fn run(cx: &Context) -> Result<()> {
    let style = Style::new();
    let mut p = Page {
        cx,
        style: &style,
        screen: Screen::Home,
        cur: 0,
        top: 0,
        account: None,
        repos: Vec::new(),
        owners: Vec::new(),
        search: String::new(),
        found: None,
        chosen: None,
        owner: String::new(),
        prefix: String::new(),
        branch: String::new(),
        env: None,
        moving: false,
        message: Vec::new(),
        quit: false,
    };
    let Some(raw) = terminal::Raw::start() else { return super::git::status(cx) };
    print!("\x1b[?1049h");
    let mut raw = Some(raw);
    let result = (|| -> Result<()> {
        if Config::load(&cx.home).is_none() {
            p.begin_connect()?;
        }
        while !p.quit {
            draw(&p.view())?;
            let Some(keys) = terminal::keys() else { break };
            for k in keys {
                p.key(k, &mut raw)?;
                if p.quit {
                    break;
                }
            }
        }
        Ok(())
    })();
    print!("\x1b[?1049l");
    drop(raw);
    result?;
    for l in &p.message {
        println!("  {l}");
    }
    Ok(())
}

impl Page<'_> {
    fn view(&mut self) -> String {
        let (width, height) = terminal::size().unwrap_or((100, 30));
        let st = self.style;
        let mut out = String::new();
        banner(&mut out, width, st, "memory backup and sync · GitHub");
        let hint = match self.screen.clone() {
            Screen::Home => {
                if let Some(c) = Config::load(&self.cx.home) {
                    let s = Status::load(&self.cx.home);
                    let vis = if c.public { st.bad("public") } else { st.ok("private") };
                    out.push_str(&format!("  {} {}  {vis}\n", st.dim("repository"), st.bold(&c.place())));
                    if !c.account.is_empty() {
                        out.push_str(&format!("  {} {}\n", st.dim("account   "), c.account));
                    }
                    for l in super::git::status_lines(&s) {
                        let (k, v) = l.split_once(": ").unwrap_or(("", l.as_str()));
                        let v = if k.starts_with("last try failed") { st.bad(v) } else { v.to_string() };
                        out.push_str(&format!("  {} {v}\n", st.dim(&fit(k, 10))));
                    }
                    out.push('\n');
                    for (i, a) in ACTIONS.iter().enumerate() {
                        let text = if i == 1 {
                            let now = if c.env { "sent" } else { "not sent" };
                            format!("{} {}", fit(a, 28), st.dim(now))
                        } else {
                            a.to_string()
                        };
                        row(&mut out, st, i == self.cur, &text);
                    }
                }
                "↑↓ move   enter choose   q quit"
            }
            Screen::Repos => {
                let title = if self.moving { "Move the memory to" } else { "Where should the memory go?" };
                out.push_str(&format!("  {}\n", st.bold(title)));
                out.push_str(&format!(
                    "  {} {}{}\n\n",
                    st.accent("⌕"),
                    self.search,
                    if self.search.is_empty() { st.dim("type to search") } else { st.accent("▏") }
                ));
                let shown = self.matching();
                let rows = height.saturating_sub(18).max(4);
                if self.cur < self.top {
                    self.top = self.cur;
                }
                if self.cur >= self.top + rows {
                    self.top = self.cur + 1 - rows;
                }
                let name_w = 44.min(width.saturating_sub(24)).max(20);
                for (r, item) in shown.iter().enumerate().skip(self.top).take(rows) {
                    let text = match item {
                        Row::New(_) => format!("{} New private repository: {}", st.accent("+"), self.new_name(*item)),
                        Row::Repo(i) => {
                            let repo = &self.repos[*i];
                            let vis = if repo.private { st.dim("Private") } else { st.bad("Public") };
                            format!("{} {vis}", fit(&repo.full_name, name_w))
                        }
                    };
                    row(&mut out, st, r == self.cur, &text);
                }
                if shown.len() > rows {
                    out.push_str(&format!("\n  {}\n", st.dim(&format!("{} of {}", self.cur + 1, shown.len()))));
                }
                "↑↓ move   enter choose   type to search   esc back"
            }
            Screen::Ask(a) => {
                let (q, about) = self.question(a);
                out.push_str(&format!("  {}\n", st.bold(&q)));
                for l in about {
                    out.push_str(&format!("  {}\n", st.dim(&l)));
                }
                out.push('\n');
                for (i, choice) in ["Yes", "No"].iter().enumerate() {
                    row(&mut out, st, i == self.cur, choice);
                }
                "↑↓ move   enter choose   y yes   n no   esc back"
            }
            Screen::Done => {
                for l in &self.message {
                    out.push_str(&format!("  {l}\n"));
                }
                "enter go on"
            }
        };
        out.push_str(&format!("\n  {}\n", st.dim(hint)));
        out
    }

    fn question(&self, a: Ask) -> (String, Vec<String>) {
        let name = self.chosen.as_ref().map(|r| r.full_name.clone()).unwrap_or_else(|| self.new_full_name());
        match a {
            Ask::SignIn => (
                "Sign in to GitHub?".into(),
                vec![
                    "No sign-in was found on this computer (GitHub CLI or git's saved one).".into(),
                    "Yes opens GitHub in the browser with a code to enter there.".into(),
                ],
            ),
            Ask::Restore => {
                let (r, ..) = self.found.as_ref().expect("found");
                let mut about =
                    vec![format!("Found in {} ({}).", r.full_name, if r.private { "private" } else { "public" })];
                if !self.cx.home.projects().is_empty() {
                    about.push("The projects on this computer are joined with it; nothing is lost.".into());
                }
                ("Restore your Paragma memory here?".into(), about)
            }
            Ask::Public => (
                format!("{name} is public. Are you sure?"),
                vec![
                    "Anyone could read the project memory there. We do not recommend it.".into(),
                    ".env files never go to a public repository.".into(),
                ],
            ),
            Ask::Branch => (
                format!("Keep the memory on its own branch ({BRANCH}) in {name}?"),
                vec![
                    format!("Recommended. The memory goes in a {FOLDER}/ folder either way."),
                    "No: on the main branch, and Paragma's commits show in its history.".into(),
                ],
            ),
            Ask::Env => (
                "Send the .env files too?".into(),
                vec![
                    "They hold the API keys kept with pg write --secret.".into(),
                    "Yes: they go as they are, so a new computer gets them back. You can change it later.".into(),
                ],
            ),
            Ask::RemoveOld => (
                "Take the memory out of the old repository?".into(),
                vec!["It stays in that repository's history either way.".into()],
            ),
            Ask::Disconnect => (
                "Stop syncing on this computer?".into(),
                vec!["The projects stay here, the memory stays in the repository.".into()],
            ),
        }
    }

    /// The rows of the repository list: a new repository of each owner
    /// first, then the repositories.
    fn matching(&self) -> Vec<Row> {
        let words: Vec<String> = self.search.to_lowercase().split_whitespace().map(str::to_string).collect();
        let fits = |text: &str| words.iter().all(|w| text.to_lowercase().contains(w.as_str()));
        let current = Config::load(&self.cx.home).map(|c| c.repo).unwrap_or_default();
        let mut out = Vec::new();
        for i in 0..self.owners.len().max(1) {
            let full = self.owner_new(i);
            // moving: not to the repository it is in
            if fits(&format!("new private repository {full}")) && !(self.moving && full.eq_ignore_ascii_case(&current))
            {
                out.push(Row::New(i));
            }
        }
        for (i, r) in self.repos.iter().enumerate() {
            if (!self.moving || r.full_name != current) && fits(&r.full_name) {
                out.push(Row::Repo(i));
            }
        }
        out
    }

    /// OWNER/paragma-memory (paragma-memory when no owner is known).
    fn owner_new(&self, i: usize) -> String {
        match self.owners.get(i) {
            Some(o) => format!("{o}/{NEW_REPO}"),
            None => NEW_REPO.to_string(),
        }
    }

    /// What a new repository row shows: its owner when there is more than
    /// one to choose from.
    fn new_name(&self, row: Row) -> String {
        match row {
            Row::New(i) if self.owners.len() > 1 => self.owner_new(i),
            _ => NEW_REPO.to_string(),
        }
    }

    /// The new repository being made, as `Account::create` takes it.
    fn new_full_name(&self) -> String {
        if self.owner.is_empty() { NEW_REPO.to_string() } else { format!("{}/{NEW_REPO}", self.owner) }
    }

    fn working(&mut self, what: &str) -> Result<()> {
        let frame = format!("{}\n  {}", self.view(), self.style.dim(what));
        draw(&frame)
    }

    fn go(&mut self, s: Screen) {
        self.cur = match s {
            // the safe answer first where it is the default
            Screen::Ask(Ask::Public | Ask::RemoveOld | Ask::Disconnect | Ask::Env) => 1,
            _ => 0,
        };
        self.top = 0;
        self.screen = s;
    }

    fn done(&mut self, lines: Vec<String>) {
        self.message = lines;
        self.go(Screen::Done);
    }

    /// Signs in, looks for the memory, then asks or lists.
    fn begin_connect(&mut self) -> Result<()> {
        self.moving = false;
        self.working("signing in to GitHub…")?;
        self.account = Account::saved()?;
        if self.account.is_none() {
            self.go(Screen::Ask(Ask::SignIn));
            return Ok(());
        }
        self.after_sign_in()
    }

    fn after_sign_in(&mut self) -> Result<()> {
        let Some(a) = self.account.take() else { return Ok(()) };
        let r = self.list_and_find(&a);
        self.account = Some(a);
        r
    }

    fn list_and_find(&mut self, a: &Account) -> Result<()> {
        self.working("reading your repositories…")?;
        self.repos = a.repos()?;
        self.owners = a.owners(&self.repos);
        if !self.moving {
            self.working("looking for your Paragma memory…")?;
            self.found = a.find_memory(&self.repos)?;
            if self.found.is_some() {
                self.go(Screen::Ask(Ask::Restore));
                return Ok(());
            }
        }
        self.search.clear();
        self.go(Screen::Repos);
        Ok(())
    }

    fn key(&mut self, k: Key, raw: &mut Option<terminal::Raw>) -> Result<()> {
        if k == Key::Quit {
            self.quit = true;
            return Ok(());
        }
        let searching = self.screen == Screen::Repos;
        let n = match &self.screen {
            Screen::Home => ACTIONS.len(),
            Screen::Repos => self.matching().len(),
            Screen::Ask(_) => 2,
            Screen::Done => 0,
        };
        match k {
            Key::Up => {
                self.cur = self.cur.saturating_sub(1);
                return Ok(());
            }
            Key::Down => {
                self.cur = (self.cur + 1).min(n.saturating_sub(1));
                return Ok(());
            }
            _ => {}
        }
        let enter = matches!(k, Key::Enter | Key::Right);
        let back = matches!(k, Key::Esc | Key::Left) || (k == Key::Char('q') && !searching);
        match self.screen.clone() {
            Screen::Home => {
                if back {
                    self.quit = true;
                } else if enter {
                    self.action()?;
                }
            }
            Screen::Done => {
                if enter || back || k == Key::Space {
                    if Config::load(&self.cx.home).is_some() {
                        self.go(Screen::Home);
                    } else {
                        self.quit = true;
                    }
                }
            }
            Screen::Repos => match k {
                Key::Esc if !self.search.is_empty() => {
                    self.search.clear();
                    self.cur = 0;
                }
                Key::Backspace if !self.search.is_empty() => {
                    self.search.pop();
                    self.cur = 0;
                }
                Key::Space => self.search.push(' '),
                Key::Char(c) if !c.is_control() => {
                    self.search.push(c);
                    self.cur = 0;
                }
                _ if back => {
                    if self.moving {
                        self.go(Screen::Home);
                    } else {
                        self.quit = true;
                    }
                }
                _ if enter && n > 0 => {
                    let item = self.matching()[self.cur];
                    self.choose(item)?;
                }
                _ => {}
            },
            Screen::Ask(a) => {
                let yes = (enter && self.cur == 0) || matches!(k, Key::Char('y') | Key::Char('Y'));
                let no = (enter && self.cur == 1) || matches!(k, Key::Char('n') | Key::Char('N'));
                if back {
                    self.ask_back(a);
                } else if yes || no {
                    self.answer(a, yes, raw)?;
                }
            }
        }
        Ok(())
    }

    fn ask_back(&mut self, a: Ask) {
        match a {
            Ask::SignIn | Ask::Restore if Config::load(&self.cx.home).is_none() => self.quit = true,
            Ask::SignIn | Ask::Restore | Ask::Disconnect => self.go(Screen::Home),
            _ => self.go(Screen::Repos),
        }
    }

    fn action(&mut self) -> Result<()> {
        match self.cur {
            0 => {
                self.working("syncing…")?;
                let lines = match sync::sync_now(&self.cx.home) {
                    Ok(s) => vec![format!("{} {}", self.style.ok("✓ synced:"), s.did)],
                    Err(e) => vec![self.style.bad(&format!("✗ {e:#}"))],
                };
                self.done(lines);
            }
            1 => {
                if let Some(mut c) = Config::load(&self.cx.home) {
                    if c.public {
                        self.done(vec![self.style.bad("The repository is public: .env files never go there.")]);
                    } else {
                        c.env = !c.env;
                        c.env_at = crate::output::now();
                        c.save(&self.cx.home)?;
                        sync::trigger(&self.cx.home);
                    }
                }
            }
            2 => {
                self.moving = true;
                self.working("signing in to GitHub…")?;
                self.account = Account::saved()?;
                if self.account.is_none() {
                    self.go(Screen::Ask(Ask::SignIn));
                } else {
                    self.after_sign_in()?;
                }
            }
            3 => self.go(Screen::Ask(Ask::Disconnect)),
            _ => self.quit = true,
        }
        Ok(())
    }

    /// A row of the repository list was chosen.
    fn choose(&mut self, row: Row) -> Result<()> {
        self.env = None;
        match row {
            Row::New(i) => {
                self.prefix = String::new();
                self.branch = "main".into();
                self.chosen = None;
                self.owner = self.owners.get(i).cloned().unwrap_or_default();
                self.go(Screen::Ask(Ask::Env));
            }
            Row::Repo(i) => {
                let repo = &self.repos[i].clone();
                self.chosen = Some(repo.clone());
                let own = repo.full_name.rsplit('/').next() == Some(NEW_REPO)
                    || self.account.as_ref().is_some_and(|a| a.is_memory(repo));
                self.prefix = if own { String::new() } else { FOLDER.to_string() };
                self.branch = repo.default_branch.clone();
                if !repo.private {
                    self.go(Screen::Ask(Ask::Public));
                } else if own {
                    self.go(Screen::Ask(Ask::Env));
                } else {
                    self.go(Screen::Ask(Ask::Branch));
                }
            }
        }
        Ok(())
    }

    fn answer(&mut self, a: Ask, yes: bool, raw: &mut Option<terminal::Raw>) -> Result<()> {
        match a {
            Ask::SignIn => {
                if !yes {
                    self.quit = Config::load(&self.cx.home).is_none();
                    if !self.quit {
                        self.go(Screen::Home);
                    }
                    self.message = vec!["Not signed in; nothing changed.".into()];
                    return Ok(());
                }
                // the terminal is the sign-in's meanwhile
                print!("\x1b[?1049l");
                *raw = None;
                println!();
                self.account = Account::sign_in()?;
                *raw = terminal::Raw::start();
                print!("\x1b[?1049h");
                if self.account.is_none() {
                    self.done(vec![self.style.bad("✗ Not signed in to GitHub.")]);
                    return Ok(());
                }
                self.after_sign_in()?;
            }
            Ask::Restore => {
                if yes {
                    let (r, prefix, branch) = self.found.clone().expect("found");
                    self.chosen = Some(r.clone());
                    self.prefix = prefix;
                    self.branch = branch;
                    // the setting the memory was sent with stays
                    self.env = None;
                    self.connect()?;
                } else {
                    self.search.clear();
                    self.go(Screen::Repos);
                }
            }
            Ask::Public => {
                if yes {
                    let own = self.prefix.is_empty();
                    if own { self.connect()? } else { self.go(Screen::Ask(Ask::Branch)) }
                } else {
                    self.go(Screen::Repos);
                }
            }
            Ask::Branch => {
                let public = self.chosen.as_ref().is_some_and(|r| !r.private);
                if yes {
                    self.branch = BRANCH.to_string();
                } else if let Some(r) = &self.chosen {
                    self.branch = r.default_branch.clone();
                }
                if public { self.connect()? } else { self.go(Screen::Ask(Ask::Env)) }
            }
            Ask::Env => {
                self.env = Some(yes);
                if self.moving {
                    self.go(Screen::Ask(Ask::RemoveOld));
                } else {
                    self.connect()?;
                }
            }
            Ask::RemoveOld => self.move_now(yes)?,
            Ask::Disconnect => {
                if yes {
                    let place = Config::load(&self.cx.home).map(|c| c.place()).unwrap_or_default();
                    sync::disconnect(&self.cx.home)?;
                    self.message =
                        vec![format!("Disconnected from {place}: the projects stay here, the memory stays there.")];
                    self.quit = true;
                } else {
                    self.go(Screen::Home);
                }
            }
        }
        Ok(())
    }

    /// The config of what was chosen; a new repository is made here.
    fn target(&mut self) -> Result<Config> {
        let new = self.new_full_name();
        if self.chosen.is_none() {
            self.working(&format!("making the private repository {new}…"))?;
        }
        let Some(a) = self.account.as_ref() else { anyhow::bail!("not signed in to GitHub") };
        let repo = match &self.chosen {
            Some(r) => r.clone(),
            None => a.create(&new)?,
        };
        Ok(super::git::config_of(a, &repo, self.prefix.clone(), self.branch.clone(), self.env))
    }

    /// Connects to what was chosen; a memory already there (restored, or
    /// found in the repository chosen) is joined with this one.
    fn connect(&mut self) -> Result<()> {
        let c = match self.target() {
            Ok(c) => c,
            Err(e) => {
                self.done(vec![self.style.bad(&format!("✗ {e:#}"))]);
                return Ok(());
            }
        };
        self.working(&format!("connecting to {}…", c.place()))?;
        let lines = match super::git::start(self.cx, &c, true) {
            Ok(l) => l.into_iter().map(|l| l.replacen("ok ", &format!("{} ", self.style.ok("✓")), 1)).collect(),
            Err(e) => vec![self.style.bad(&format!("✗ {e:#}"))],
        };
        self.done(lines);
        Ok(())
    }

    fn move_now(&mut self, remove_old: bool) -> Result<()> {
        let Some(old) = Config::load(&self.cx.home) else { return Ok(()) };
        let new = match self.target() {
            Ok(c) => c,
            Err(e) => {
                self.done(vec![self.style.bad(&format!("✗ {e:#}"))]);
                return Ok(());
            }
        };
        self.working(&format!("moving the memory to {}…", new.place()))?;
        let lines = match super::git::move_memory(self.cx, &old, &new, remove_old) {
            Ok(l) => l.into_iter().map(|l| l.replacen("ok ", &format!("{} ", self.style.ok("✓")), 1)).collect(),
            Err(e) => vec![self.style.bad(&format!("✗ {e:#}"))],
        };
        self.moving = false;
        self.done(lines);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str, private: bool) -> Repo {
        Repo {
            full_name: name.into(),
            private,
            default_branch: "main".into(),
            clone_url: format!("https://github.com/{name}.git"),
            own: true,
        }
    }

    fn page<'a>(cx: &'a Context, style: &'a Style) -> Page<'a> {
        Page {
            cx,
            style,
            screen: Screen::Repos,
            cur: 0,
            top: 0,
            account: None,
            repos: vec![repo("me/notes-app", true), repo("acme-org/billing", true), repo("me/website", false)],
            owners: vec!["me".into(), "acme-org".into()],
            search: String::new(),
            found: None,
            chosen: None,
            owner: String::new(),
            prefix: String::new(),
            branch: String::new(),
            env: None,
            moving: false,
            message: Vec::new(),
            quit: false,
        }
    }

    #[test]
    fn the_list_says_public_and_asks_before_it() {
        let d = tempfile::tempdir().unwrap();
        let cx = Context {
            home: crate::project::Home { root: d.path().to_path_buf() },
            project: None,
            budget: 1200,
            cwd: d.path().to_path_buf(),
        };
        let style = Style::plain();
        let mut p = page(&cx, &style);
        let list = p.view();
        // a new repository of the account or of its organization
        assert!(list.contains("New private repository: me/paragma-memory"), "{list}");
        assert!(list.contains("New private repository: acme-org/paragma-memory"), "{list}");
        assert!(list.contains("me/notes-app") && list.contains("acme-org/billing"), "{list}");
        let public = list.lines().find(|l| l.contains("me/website")).unwrap();
        assert!(public.contains("Public"), "{public}");

        // typing searches
        let mut raw = None;
        for c in "billing".chars() {
            p.key(Key::Char(c), &mut raw).unwrap();
        }
        assert_eq!(p.matching(), vec![Row::Repo(1)]);
        p.key(Key::Esc, &mut raw).unwrap();
        for c in "acme".chars() {
            p.key(Key::Char(c), &mut raw).unwrap();
        }
        assert_eq!(p.matching(), vec![Row::New(1), Row::Repo(1)]);
        p.key(Key::Esc, &mut raw).unwrap();

        // a public repository: asked first, and "no" is where the cursor starts
        p.choose(Row::Repo(2)).unwrap();
        assert_eq!(p.screen, Screen::Ask(Ask::Public));
        assert_eq!(p.cur, 1, "the safe answer first");
        let ask = p.view();
        assert!(ask.contains("is public. Are you sure?") && ask.contains("We do not recommend it"), "{ask}");
        p.key(Key::Char('n'), &mut raw).unwrap();
        assert_eq!(p.screen, Screen::Repos);

        // a repository with other things: its own branch is asked, then the .env files
        p.choose(Row::Repo(0)).unwrap();
        assert_eq!(p.screen, Screen::Ask(Ask::Branch));
        assert_eq!(p.prefix, "Paragma");
        assert_eq!(p.cur, 0, "its own branch is recommended");
        p.key(Key::Char('y'), &mut raw).unwrap();
        assert_eq!(p.branch, "paragma");
        assert_eq!(p.screen, Screen::Ask(Ask::Env));
        assert_eq!(p.cur, 1, ".env files are not sent unless chosen");
        p.key(Key::Esc, &mut raw).unwrap();

        // a new repository: the projects at its root, the .env question next
        p.choose(Row::New(0)).unwrap();
        assert!(p.prefix.is_empty() && p.screen == Screen::Ask(Ask::Env));
        assert_eq!(p.new_full_name(), "me/paragma-memory");
        // in the organization
        p.choose(Row::New(1)).unwrap();
        assert_eq!(p.new_full_name(), "acme-org/paragma-memory");

        // one owner: the row says only the name
        p.owners.truncate(1);
        p.go(Screen::Repos);
        let one = p.view();
        assert!(one.contains("New private repository: paragma-memory") && !one.contains("me/paragma"), "{one}");
    }
}
