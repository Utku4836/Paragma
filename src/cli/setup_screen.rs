//! The screen of `pg setup` in a terminal: choose an agent, then where
//! Paragma goes (every conversation, one conversation's folder, this
//! folder), then confirm. Arrow keys move, Enter goes on, Esc goes back.

use super::Context;
use super::setup::{Style, connect};
use crate::agents::{self, Agent, Conversation};
use crate::output;
use crate::terminal::{self, Key};
use anyhow::Result;
use std::io::Write;
use std::path::PathBuf;

const BANNER: [&str; 6] = [
    "██████╗  █████╗ ██████╗  █████╗  ██████╗ ███╗   ███╗ █████╗ ",
    "██╔══██╗██╔══██╗██╔══██╗██╔══██╗██╔════╝ ████╗ ████║██╔══██╗",
    "██████╔╝███████║██████╔╝███████║██║  ███╗██╔████╔██║███████║",
    "██╔═══╝ ██╔══██║██╔══██╗██╔══██║██║   ██║██║╚██╔╝██║██╔══██║",
    "██║     ██║  ██║██║  ██║██║  ██║╚██████╔╝██║ ╚═╝ ██║██║  ██║",
    "╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝ ╚═╝     ╚═╝╚═╝  ╚═╝",
];

/// The choices on the scope screen, in order.
const SCOPES: [&str; 4] = ["Every conversation", "One conversation", "This folder", "Back"];

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Agents,
    Scope,
    Conversations,
    Confirm,
}

struct Setup<'a> {
    style: &'a Style,
    cwd: PathBuf,
    agents: Vec<Agent>,
    screen: Screen,
    /// The highlighted row of the current list.
    cur: usize,
    /// The first conversation shown, when they do not all fit.
    top: usize,
    agent: usize,
    conversations: Vec<Conversation>,
    /// Typed on the conversations screen: only those matching are listed.
    search: String,
    target: PathBuf,
    /// Where, for people: "every conversation", "~/code/app".
    place: String,
    /// The screen Esc on the confirm screen goes back to.
    back: Screen,
    message: Option<String>,
    /// What was set up, said again after the screen closes.
    done: Vec<String>,
    quit: bool,
}

/// Runs the screen; false when the terminal cannot read keys one at a time.
pub fn run(cx: &Context, style: &Style) -> Result<bool> {
    let agents: Vec<Agent> = agents::known().into_iter().filter(|a| a.installed).collect();
    let mut s = Setup {
        style,
        cwd: cx.cwd.clone(),
        agents,
        screen: Screen::Agents,
        cur: 0,
        top: 0,
        agent: 0,
        conversations: Vec::new(),
        search: String::new(),
        target: PathBuf::new(),
        place: String::new(),
        back: Screen::Agents,
        message: None,
        done: Vec::new(),
        quit: false,
    };
    {
        let Some(_raw) = terminal::Raw::start() else { return Ok(false) };
        print!("\x1b[?1049h"); // a screen of its own, the terminal's text comes back after
        let result = (|| -> Result<()> {
            while !s.quit {
                draw(&s.view())?;
                let Some(keys) = terminal::keys() else { break };
                for k in keys {
                    s.key(k)?;
                }
            }
            Ok(())
        })();
        print!("\x1b[?1049l");
        result?;
    }
    s.summary();
    Ok(true)
}

/// Draws over the last frame: each line is written over the old one and
/// cleared after, so the screen never goes blank between frames.
fn draw(frame: &str) -> Result<()> {
    let mut out = std::io::stdout().lock();
    // raw mode on Unix does not return the carriage at a line end
    let body = frame.replace('\n', "\x1b[K\r\n");
    write!(out, "\x1b[H{body}\x1b[K\x1b[J")?;
    out.flush()?;
    Ok(())
}

/// `s` cut or padded to `n` characters.
fn fit(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count > n {
        let cut: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        format!("{s}{}", " ".repeat(n - count))
    }
}

impl Setup<'_> {
    fn size(&self) -> (usize, usize) {
        terminal::size().unwrap_or((100, 30))
    }

    fn banner(&self, out: &mut String, width: usize) {
        let st = self.style;
        out.push('\n');
        if width < BANNER[0].chars().count() + 4 {
            out.push_str(&format!("  {}\n", st.accent("◆ PARAGMA")));
        } else {
            for row in BANNER {
                // the blocks in violet, their box-drawn shadow darker
                let mut line = String::from("  ");
                let mut run = String::new();
                let mut solid = None;
                for ch in row.chars() {
                    let is_solid = ch == '█';
                    if solid != Some(is_solid) && !run.is_empty() {
                        line.push_str(&if solid == Some(true) { st.accent(&run) } else { st.shadow(&run) });
                        run.clear();
                    }
                    solid = Some(is_solid);
                    run.push(ch);
                }
                line.push_str(&if solid == Some(true) { st.accent(&run) } else { st.shadow(&run) });
                out.push_str(&line);
                out.push('\n');
            }
        }
        out.push_str(&format!(
            "  {}\n\n",
            st.dim(&format!("memory for coding agents · v{}", env!("CARGO_PKG_VERSION")))
        ));
    }

    /// One row of a list: highlighted when chosen.
    fn row(&self, out: &mut String, chosen: bool, text: &str) {
        if chosen {
            out.push_str(&format!("  {} {}\n", self.style.accent("›"), self.style.bold(text)));
        } else {
            out.push_str(&format!("    {text}\n"));
        }
    }

    fn view(&mut self) -> String {
        let (width, height) = self.size();
        let st = self.style;
        let mut out = String::new();
        self.banner(&mut out, width);
        let hint = match self.screen {
            Screen::Agents => {
                out.push_str(&format!("  {}\n\n", st.bold("Connect Paragma to an agent")));
                if self.agents.is_empty() {
                    out.push_str(&format!(
                        "  {}\n",
                        st.dim("No agent found on this computer (Claude Code, Codex, Antigravity, OpenCode).")
                    ));
                }
                for (i, a) in self.agents.iter().enumerate() {
                    let state = if crate::skills::installed(&a.global) {
                        format!("  {}", st.ok("✓ every conversation"))
                    } else {
                        String::new()
                    };
                    let apps = fit(&a.apps.join(" · "), 16);
                    let line = format!("{} {}{state}", fit(a.name, 13), st.dim(&apps));
                    self.row(&mut out, i == self.cur, &line);
                }
                "↑↓ move   enter choose   q quit"
            }
            Screen::Scope => {
                let a = &self.agents[self.agent];
                out.push_str(&format!("  {}  {}\n\n", st.bold(a.name), st.dim("where should Paragma be read?")));
                let about = [
                    agents::short(&a.global),
                    format!("pick one; Paragma goes into its folder ({})", a.folder_file),
                    agents::short(&self.cwd.join(a.folder_file)),
                    String::new(),
                ];
                for (i, (name, about)) in SCOPES.iter().zip(&about).enumerate() {
                    let line = format!("{} {}", fit(name, 20), st.dim(about));
                    self.row(&mut out, i == self.cur, &line);
                }
                "↑↓ move   enter choose   esc back"
            }
            Screen::Conversations => {
                let a = &self.agents[self.agent];
                out.push_str(&format!(
                    "  {}  {}\n",
                    st.bold(a.name),
                    st.dim("pick a conversation: Paragma goes into its folder, for every conversation there")
                ));
                out.push_str(&format!(
                    "  {} {}{}\n\n",
                    st.accent("⌕"),
                    self.search,
                    if self.search.is_empty() { st.dim("type to search") } else { st.accent("▏") }
                ));
                let shown = self.matching();
                if shown.is_empty() {
                    let none = if self.conversations.is_empty() { "No conversations found." } else { "None match." };
                    out.push_str(&format!("  {}\n", st.dim(none)));
                }
                let rows = height.saturating_sub(19).max(4);
                if self.cur < self.top {
                    self.top = self.cur;
                }
                if self.cur >= self.top + rows {
                    self.top = self.cur + 1 - rows;
                }
                let title_w = 40.min(width.saturating_sub(40)).max(16);
                let when_w = 12;
                let folder_w = width.saturating_sub(title_w + when_w + 12).max(12);
                let now = output::now();
                for (row, &i) in shown.iter().enumerate().skip(self.top).take(rows) {
                    let c = &self.conversations[i];
                    let set = crate::skills::installed(&c.folder.join(a.folder_file));
                    let mark = if set { st.ok("✓") } else { " ".into() };
                    let line = format!(
                        "{} {mark} {} {}",
                        fit(&c.title, title_w),
                        st.dim(&fit(&agents::short(&c.folder), folder_w)),
                        st.dim(&fit(&output::ago(c.updated, now), when_w))
                    );
                    self.row(&mut out, row == self.cur, &line);
                }
                if shown.len() > rows {
                    out.push_str(&format!("\n  {}\n", st.dim(&format!("{} of {}", self.cur + 1, shown.len()))));
                }
                "↑↓ move   enter choose   type to search   esc back"
            }
            Screen::Confirm => {
                let a = &self.agents[self.agent];
                out.push_str(&format!(
                    "  {}\n\n",
                    st.bold(&format!("Set up Paragma for {} in {}?", a.name, self.place))
                ));
                out.push_str(&format!(
                    "  {} {}\n",
                    st.dim("Adds a \"Memory (Paragma)\" section to"),
                    agents::short(&self.target)
                ));
                out.push_str(&format!(
                    "  {}\n",
                    st.dim("The rest of the file stays as it is; running setup again only updates the section.")
                ));
                "enter / y set up   esc / n back"
            }
        };
        if let Some(m) = &self.message {
            out.push_str(&format!("\n  {m}\n"));
        }
        out.push_str(&format!("\n  {}\n", st.dim(hint)));
        out
    }

    /// The conversations the search matches, as indexes.
    fn matching(&self) -> Vec<usize> {
        let words: Vec<String> = self.search.to_lowercase().split_whitespace().map(str::to_string).collect();
        (0..self.conversations.len())
            .filter(|&i| {
                let c = &self.conversations[i];
                let text = format!("{} {}", c.title, c.folder.to_string_lossy()).to_lowercase();
                words.iter().all(|w| text.contains(w.as_str()))
            })
            .collect()
    }

    fn rows(&self) -> usize {
        match self.screen {
            Screen::Agents => self.agents.len(),
            Screen::Scope => SCOPES.len(),
            Screen::Conversations => self.matching().len(),
            Screen::Confirm => 0,
        }
    }

    fn go(&mut self, screen: Screen, cur: usize) {
        self.screen = screen;
        self.cur = cur;
        self.top = 0;
    }

    fn key(&mut self, k: Key) -> Result<()> {
        if k == Key::Quit {
            self.quit = true;
            return Ok(());
        }
        let searching = self.screen == Screen::Conversations;
        let n = self.rows();
        match k {
            Key::Up => {
                self.cur = self.cur.saturating_sub(1);
                return Ok(());
            }
            Key::Down => {
                self.cur = (self.cur + 1).min(n.saturating_sub(1));
                return Ok(());
            }
            Key::Char('k') if !searching => return self.key(Key::Up),
            Key::Char('j') if !searching => return self.key(Key::Down),
            _ => {}
        }
        let back = matches!(k, Key::Esc | Key::Left | Key::Backspace) || (k == Key::Char('q') && !searching);
        let enter = matches!(k, Key::Enter | Key::Right);
        match self.screen {
            Screen::Agents => {
                if matches!(k, Key::Esc | Key::Char('q')) {
                    self.quit = true;
                } else if enter && n > 0 {
                    self.agent = self.cur;
                    self.message = None;
                    self.go(Screen::Scope, 0);
                }
            }
            Screen::Scope => {
                let a = &self.agents[self.agent];
                let (global, folder_file) = (a.global.clone(), a.folder_file);
                match (back, enter, self.cur) {
                    (true, ..) | (_, true, 3) => self.go(Screen::Agents, self.agent),
                    (_, true, 0) => {
                        self.target = global;
                        self.place = "every conversation".into();
                        self.back = Screen::Scope;
                        self.go(Screen::Confirm, 0);
                    }
                    (_, true, 1) => {
                        draw(&format!("{}\n  {}", self.view(), self.style.dim("reading conversations…")))?;
                        self.conversations = self.agents[self.agent].conversations();
                        self.search.clear();
                        self.go(Screen::Conversations, 0);
                    }
                    (_, true, 2) => {
                        self.target = self.cwd.join(folder_file);
                        self.place = agents::short(&self.cwd);
                        self.back = Screen::Scope;
                        self.go(Screen::Confirm, 0);
                    }
                    _ => {}
                }
            }
            Screen::Conversations => match k {
                Key::Esc if !self.search.is_empty() => self.search_changed(String::new()),
                Key::Backspace if !self.search.is_empty() => {
                    let mut s = self.search.clone();
                    s.pop();
                    self.search_changed(s);
                }
                Key::Space => self.search_changed(format!("{} ", self.search)),
                Key::Char(c) if !c.is_control() => self.search_changed(format!("{}{c}", self.search)),
                _ if back => self.go(Screen::Scope, 1),
                _ if enter && n > 0 => {
                    let c = &self.conversations[self.matching()[self.cur]];
                    let a = &self.agents[self.agent];
                    self.target = c.folder.join(a.folder_file);
                    self.place = agents::short(&c.folder);
                    self.back = Screen::Conversations;
                    self.screen = Screen::Confirm;
                }
                _ => {}
            },
            Screen::Confirm => {
                if enter || k == Key::Char('y') || k == Key::Char('Y') {
                    self.install();
                } else if back || k == Key::Char('n') || k == Key::Char('N') {
                    let cur = if self.back == Screen::Scope { self.cur_scope() } else { self.cur };
                    self.screen = self.back;
                    self.cur = cur;
                }
            }
        }
        Ok(())
    }

    fn search_changed(&mut self, s: String) {
        self.search = s;
        self.cur = 0;
        self.top = 0;
    }

    /// The scope row that led to the confirm screen.
    fn cur_scope(&self) -> usize {
        if self.target == self.agents[self.agent].global { 0 } else { 2 }
    }

    fn install(&mut self) {
        let a = &self.agents[self.agent];
        self.message = Some(match connect(self.style, a, &self.target) {
            Ok(line) => {
                self.done.push(line.clone());
                line.trim_start().to_string()
            }
            Err(e) => self.style.bad(&format!("✗ {}: {e}", a.name)),
        });
        let agent = self.agent;
        self.go(Screen::Agents, agent);
    }

    /// What was set up, after the screen closes.
    fn summary(&self) {
        let st = self.style;
        println!();
        if self.done.is_empty() {
            println!("  {} {}", st.accent("◆ Paragma"), st.dim("nothing was set up"));
            println!();
            return;
        }
        println!("  {}", st.accent("◆ Paragma"));
        for line in &self.done {
            println!("{line}");
        }
        println!();
        println!(
            "  {} In a conversation, ask for work: the agent opens a project with {}.",
            st.ok("Ready."),
            st.bold("pg open")
        );
        println!("  {}", st.dim("Skills: pg skill coding | projects | knowledge"));
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_cuts_and_pads() {
        assert_eq!(fit("abc", 5), "abc  ");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("şğü", 3), "şğü");
    }
}
