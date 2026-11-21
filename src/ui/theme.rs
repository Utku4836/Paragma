//! The interface's colours and marks: one accent (Paragma's violet), quiet
//! greys, green for done and red for trouble, on the terminal's own
//! background. The marks are all one family (geometric shapes), one cell
//! wide, and none a terminal draws as a coloured emoji (☑, ⚙, ⚠ are).

use ratatui::style::{Color, Modifier, Style};

pub const ACCENT: Color = Color::Rgb(167, 139, 250);
pub const ACCENT_DEEP: Color = Color::Rgb(124, 58, 237);
pub const TEXT: Color = Color::Rgb(226, 226, 232);
pub const DIM: Color = Color::Rgb(150, 150, 162);
pub const FAINT: Color = Color::Rgb(88, 88, 104);
pub const OK: Color = Color::Rgb(110, 207, 150);
pub const BAD: Color = Color::Rgb(248, 113, 113);
pub const WARN: Color = Color::Rgb(251, 191, 36);
pub const CODE: Color = Color::Rgb(147, 197, 253);
pub const LINK: Color = Color::Rgb(125, 211, 252);
/// Behind the row the mouse is over, and the line the cursor is on.
pub const HOVER_BG: Color = Color::Rgb(38, 38, 52);
/// Behind the chosen row of a list that does not have the keys.
pub const IDLE_BG: Color = Color::Rgb(52, 44, 80);

/// The marks of the sections and of what is in them.
pub mod mark {
    pub const PROJECTS: &str = "◆";
    pub const NOTES: &str = "▤";
    pub const PLAN: &str = "◧";
    pub const FILES: &str = "▣";
    pub const ACTIVITY: &str = "◷";
    pub const SYNC: &str = "◎";
    pub const SETTINGS: &str = "◈";
    pub const FOLDER_OPEN: &str = "▾";
    pub const FOLDER: &str = "▸";
    pub const NOTE: &str = "·";
    pub const TODO: &str = "○";
    pub const DONE: &str = "●";
    pub const LATER: &str = "◌";
    pub const QUESTION: &str = "?";
    pub const HANDOFF: &str = "↳";
    pub const CURRENT: &str = "●";
    pub const BAR: &str = "▌";
    pub const WARN: &str = "!";
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
/// The chosen row of the list that has the keys: impossible to miss.
pub fn chosen() -> Style {
    Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
}
/// The chosen row of a list that does not have the keys now.
pub fn chosen_idle() -> Style {
    Style::default().fg(TEXT).bg(IDLE_BG)
}
/// Selected text in the editor.
pub fn selected() -> Style {
    Style::default().fg(Color::White).bg(ACCENT_DEEP)
}
pub fn border(focused: bool) -> Style {
    if focused { Style::default().fg(ACCENT) } else { Style::default().fg(FAINT) }
}
pub fn key() -> Style {
    Style::default().fg(Color::Black).bg(DIM)
}
