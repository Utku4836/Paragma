//! The interface's colours: one accent (Paragma's violet), quiet greys,
//! green for done and red for trouble, on the terminal's own background.

use ratatui::style::{Color, Modifier, Style};

pub const ACCENT: Color = Color::Rgb(167, 139, 250);
pub const ACCENT_DEEP: Color = Color::Rgb(124, 58, 237);
pub const TEXT: Color = Color::Rgb(226, 226, 232);
pub const DIM: Color = Color::Rgb(138, 138, 150);
pub const FAINT: Color = Color::Rgb(78, 78, 92);
pub const OK: Color = Color::Rgb(110, 207, 150);
pub const BAD: Color = Color::Rgb(248, 113, 113);
pub const WARN: Color = Color::Rgb(251, 191, 36);
pub const SELECT_BG: Color = Color::Rgb(58, 46, 92);
pub const CODE: Color = Color::Rgb(147, 197, 253);
pub const LINK: Color = Color::Rgb(125, 211, 252);

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
/// The chosen row of a list.
pub fn chosen() -> Style {
    Style::default().fg(TEXT).bg(SELECT_BG).add_modifier(Modifier::BOLD)
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
