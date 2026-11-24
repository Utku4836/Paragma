//! The interface for people: `pg` alone, in a terminal. Projects, notes
//! (read formatted, written in an editor with the usual keys), the plan,
//! the files kept with notes, what agents and pg did, sync and settings, by
//! keyboard and mouse. Every change is a pg command or the same code the
//! commands use: agents see it as they see each other's work.

pub mod app;
pub mod data;
pub mod draw;
pub mod editor;
pub mod folders;
pub mod markdown;
pub mod panels;
pub mod theme;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::crossterm::{execute, terminal};
use std::io::stdout;
use std::time::Duration;

fn enter() -> Result<()> {
    terminal::enable_raw_mode()?;
    execute!(
        stdout(),
        terminal::EnterAlternateScreen,
        event::EnableMouseCapture,
        event::EnableBracketedPaste,
        event::EnableFocusChange
    )?;
    Ok(())
}

fn leave() {
    let _ = execute!(
        stdout(),
        ratatui::crossterm::cursor::SetCursorStyle::DefaultUserShape,
        event::DisableFocusChange,
        event::DisableBracketedPaste,
        event::DisableMouseCapture,
        terminal::LeaveAlternateScreen
    );
    let _ = terminal::disable_raw_mode();
}

pub fn run(home: crate::project::Home, project: Option<crate::project::Project>) -> Result<()> {
    // the terminal comes back as it was, also after a crash
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave();
        hook(info);
    }));
    enter()?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout());
    let mut term = ratatui::Terminal::new(backend)?;
    term.clear()?;
    let mut app = app::App::new(home, project);
    let result = (|| -> Result<()> {
        while !app.quit {
            term.draw(|f| draw::draw(f, &mut app))?;
            // every event waiting is taken before the next frame: keys held
            // down or a wheel turned fast do not queue up behind drawing
            let wait = if app.jobs.is_empty() { 250 } else { 80 };
            if event::poll(Duration::from_millis(wait))? {
                loop {
                    match event::read()? {
                        // Windows sends a release too: a key counts once
                        Event::Key(k) if k.kind != KeyEventKind::Release => app.key(k),
                        Event::Mouse(m) => app.mouse(m),
                        Event::Paste(t) => app.paste(&t),
                        Event::FocusGained => app.reload(),
                        _ => {}
                    }
                    if app.quit || app.run_outside.is_some() || !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
            // a bar cursor while writing, as editors have it
            let style = if app.open.as_ref().is_some_and(|o| !o.reading) || app.modal.is_some() {
                ratatui::crossterm::cursor::SetCursorStyle::BlinkingBar
            } else {
                ratatui::crossterm::cursor::SetCursorStyle::DefaultUserShape
            };
            let _ = execute!(stdout(), style);
            // the background's answers, the search typed, what agents changed
            app.tick();
            if app.quit && app.unsaved() {
                app.quit = false;
                app.modal = Some(app::Modal::Confirm {
                    what: app::Confirm::QuitUnsaved,
                    title: "Quit without saving?".into(),
                    lines: vec!["A note open has changes that are not saved.".into()],
                });
            }
            // signing in to GitHub: the terminal is GitHub's meanwhile
            if let Some(out) = app.run_outside.take() {
                leave();
                let app::Outside::SignIn(moving) = out;
                println!("Signing in to GitHub for Paragma's sync…");
                let ok = matches!(crate::sync::github::Account::sign_in(), Ok(Some(_)));
                enter()?;
                term.clear()?;
                app.signed_in(ok, moving);
            }
        }
        Ok(())
    })();
    leave();
    // a sync or a setup still running ends before pg does
    app.finish_jobs();
    result
}
