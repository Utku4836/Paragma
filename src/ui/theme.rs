//! The interface's colours and marks: Paragma's violet for what matters
//! (headings, the chosen row, icons, titles), as the colour of letters;
//! the keys at the top and the bottom are the only boxes of colour. Quiet
//! greys for the rest, green for done and red for trouble, on the
//! terminal's own background. No mark is one a terminal draws as a
//! coloured emoji (☑, ⚙, ⚠, ▶ are).

use ratatui::style::{Color, Modifier, Style};

pub const ACCENT: Color = Color::Rgb(167, 139, 250);
pub const ACCENT_DEEP: Color = Color::Rgb(124, 58, 237);
/// The frames and lines of what does not have the keys: violet, dark.
pub const FRAME: Color = Color::Rgb(84, 66, 140);
pub const TEXT: Color = Color::Rgb(226, 226, 232);
pub const DIM: Color = Color::Rgb(160, 156, 178);
pub const FAINT: Color = Color::Rgb(98, 92, 122);
pub const OK: Color = Color::Rgb(110, 207, 150);
pub const BAD: Color = Color::Rgb(248, 113, 113);
pub const WARN: Color = Color::Rgb(251, 191, 36);
pub const CODE: Color = Color::Rgb(147, 197, 253);
pub const LINK: Color = Color::Rgb(125, 211, 252);
/// Behind the row the mouse is over, and the line the cursor is on: a
/// quiet grey, not a box of colour.
pub const HOVER_BG: Color = Color::Rgb(36, 36, 42);

/// The marks of the sections and of what is in them.
pub mod mark {
    pub const HOME: &str = "⌂";
    pub const PROJECTS: &str = "◆";
    pub const NOTES: &str = "■";
    pub const PLAN: &str = "▲";
    pub const FILES: &str = "▣";
    pub const AGENTS: &str = "●";
    pub const ACTIVITY: &str = "≡";
    pub const SYNC: &str = "⇅";
    pub const SETUP: &str = "✦";
    pub const FOLDER_OPEN: &str = "▾";
    pub const FOLDER: &str = "▸";
    pub const NOTE: &str = "·";
    pub const TODO: &str = "○";
    pub const DONE: &str = "●";
    pub const LATER: &str = "◌";
    pub const QUESTION: &str = "?";
    pub const HANDOFF: &str = "↳";
    pub const CURRENT: &str = "●";
    /// Before the chosen row.
    pub const BAR: &str = "›";
    pub const WARN: &str = "!";
    /// Changed here, not sent yet.
    pub const UNSENT: &str = "↑";
    pub const ON: &str = "●";
    pub const OFF: &str = "○";
}

pub fn text() -> Style {
    Style::default().fg(TEXT)
}
pub fn dim() -> Style {
    Style::default().fg(DIM)
}
pub fn faint() -> Style {
    Style::default().fg(FAINT)
}
pub fn accent() -> Style {
    Style::default().fg(ACCENT)
}
pub fn heading() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}
pub fn ok() -> Style {
    Style::default().fg(OK)
}
pub fn bad() -> Style {
    Style::default().fg(BAD)
}
pub fn warn() -> Style {
    Style::default().fg(WARN)
}
pub fn code() -> Style {
    Style::default().fg(CODE)
}
pub fn link() -> Style {
    Style::default().fg(LINK).add_modifier(Modifier::UNDERLINED)
}
/// The chosen row of the list that has the keys: its letters violet and
/// bold, with › before it; no box of colour.
pub fn chosen() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}
/// The chosen row of a list that does not have the keys now.
pub fn chosen_idle() -> Style {
    Style::default().fg(ACCENT)
}
/// Selected text in the editor.
pub fn selected() -> Style {
    Style::default().fg(Color::White).bg(ACCENT_DEEP)
}
pub fn border(focused: bool) -> Style {
    if focused { Style::default().fg(ACCENT) } else { Style::default().fg(FRAME) }
}
/// A key in the hints at the top and the bottom: a violet pill (the only
/// boxes of colour).
pub fn key() -> Style {
    Style::default().fg(Color::White).bg(ACCENT_DEEP).add_modifier(Modifier::BOLD)
}
/// A button, a tab, a title: words, the main one violet.
pub fn button() -> Style {
    Style::default().fg(DIM)
}
pub fn button_main() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}
/// A section's icon: violet, bold when it is the chosen section.
pub fn tile(lit: bool) -> Style {
    if lit { Style::default().fg(ACCENT).add_modifier(Modifier::BOLD) } else { Style::default().fg(ACCENT) }
}
