//! The terminal, for screens that read keys: raw input (a key at a time, no
//! echo), its size, and colours on Windows consoles. Raw input is undone
//! when the `Raw` guard is dropped, also after an error.

use std::io::Read;

/// A key pressed.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
    Backspace,
    Space,
    Quit,
    Char(char),
    Other,
}

/// Reads the keys pressed (several when typed fast); None when input has ended.
pub fn keys() -> Option<Vec<Key>> {
    let mut buf = [0u8; 64];
    let n = std::io::stdin().read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    Some(parse(&buf[..n]))
}

/// Splits what was read into keys.
fn parse(mut b: &[u8]) -> Vec<Key> {
    let mut out = Vec::new();
    while !b.is_empty() {
        let len = match b {
            // an escape sequence ends with a byte from @ to ~
            [0x1b, b'[' | b'O', rest @ ..] => {
                2 + rest.iter().position(|c| (0x40..=0x7e).contains(c)).map_or(rest.len(), |i| i + 1)
            }
            [b'\r', b'\n', ..] => 2,
            [c, ..] if *c >= 0x80 => match std::str::from_utf8(&b[..b.len().min(4)]) {
                Ok(s) => s.chars().next().map_or(1, char::len_utf8),
                Err(e) => e.valid_up_to().max(1),
            },
            _ => 1,
        };
        let (one, rest) = b.split_at(len.min(b.len()));
        out.push(name(one));
        b = rest;
    }
    out
}

fn name(b: &[u8]) -> Key {
    match b {
        b"\x1b[A" | b"\x1bOA" => Key::Up,
        b"\x1b[B" | b"\x1bOB" => Key::Down,
        b"\x1b[C" | b"\x1bOC" => Key::Right,
        b"\x1b[D" | b"\x1bOD" => Key::Left,
        b"\r" | b"\n" | b"\r\n" => Key::Enter,
        b"\x1b" => Key::Esc,
        b"\x7f" | b"\x08" => Key::Backspace,
        b" " => Key::Space,
        b"\x03" => Key::Quit,
        _ => match std::str::from_utf8(b).ok().and_then(|s| {
            let mut c = s.chars();
            c.next().filter(|_| c.next().is_none())
        }) {
            Some(c) => Key::Char(c),
            None => Key::Other,
        },
    }
}

/// Columns and rows, when known.
pub fn size() -> Option<(usize, usize)> {
    sys::size()
}

/// Raw input while alive; also hides the cursor.
pub struct Raw {
    saved: sys::Saved,
}

impl Raw {
    pub fn start() -> Option<Raw> {
        let saved = sys::raw()?;
        print!("\x1b[?25l");
        Some(Raw { saved })
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        print!("\x1b[?25h");
        use std::io::Write;
        let _ = std::io::stdout().flush();
        sys::restore(&self.saved);
    }
}

/// Lets a Windows console show colours; true elsewhere.
pub fn enable_colours() -> bool {
    sys::enable_colours()
}

#[cfg(windows)]
mod sys {
    use windows_sys::Win32::System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
        ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetConsoleScreenBufferInfo,
        GetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
    };

    pub struct Saved(u32);

    pub fn raw() -> Option<Saved> {
        unsafe {
            let h = GetStdHandle(STD_INPUT_HANDLE);
            let mut mode = 0;
            if GetConsoleMode(h, &mut mode) == 0 {
                return None;
            }
            let new = (mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
                | ENABLE_VIRTUAL_TERMINAL_INPUT;
            (SetConsoleMode(h, new) != 0).then_some(Saved(mode))
        }
    }

    pub fn restore(s: &Saved) {
        unsafe {
            SetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), s.0);
        }
    }

    pub fn size() -> Option<(usize, usize)> {
        unsafe {
            let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
            if GetConsoleScreenBufferInfo(GetStdHandle(STD_OUTPUT_HANDLE), &mut info) == 0 {
                return None;
            }
            let w = info.srWindow.Right - info.srWindow.Left + 1;
            let h = info.srWindow.Bottom - info.srWindow.Top + 1;
            (w > 0 && h > 0).then_some((w as usize, h as usize))
        }
    }

    pub fn enable_colours() -> bool {
        unsafe {
            let h = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut mode = 0;
            GetConsoleMode(h, &mut mode) != 0 && SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
        }
    }
}

#[cfg(not(windows))]
mod sys {
    use std::process::{Command, Stdio};

    /// What `stty -g` printed: the settings to go back to.
    pub struct Saved(String);

    fn stty(args: &[&str]) -> Option<String> {
        let o = Command::new("stty").args(args).stdin(Stdio::inherit()).stderr(Stdio::null()).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    pub fn raw() -> Option<Saved> {
        let saved = stty(&["-g"])?;
        stty(&["raw", "-echo"])?;
        Some(Saved(saved))
    }

    pub fn restore(s: &Saved) {
        stty(&[&s.0]);
    }

    pub fn size() -> Option<(usize, usize)> {
        let s = stty(&["size"])?;
        let mut it = s.split_whitespace().filter_map(|n| n.parse::<usize>().ok());
        let (rows, cols) = (it.next()?, it.next()?);
        (cols > 0 && rows > 0).then_some((cols, rows))
    }

    pub fn enable_colours() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_named() {
        assert_eq!(name(b"\x1b[A"), Key::Up);
        assert_eq!(name(b"\x1bOB"), Key::Down);
        assert_eq!(name(b"\r"), Key::Enter);
        assert_eq!(name(b"\x1b"), Key::Esc);
        assert_eq!(name(b"\x03"), Key::Quit);
        assert_eq!(name(b"y"), Key::Char('y'));
        assert_eq!(name("ş".as_bytes()), Key::Char('ş'));
        assert_eq!(name(b"\x1b[5~"), Key::Other);
        assert_eq!(parse(b"\x1b[A\x1b[Bq\r\n"), [Key::Up, Key::Down, Key::Char('q'), Key::Enter]);
        assert_eq!(parse("aş\x1b".as_bytes()), [Key::Char('a'), Key::Char('ş'), Key::Esc]);
    }
}
