//! What the full-screen pages of pg (setup, git) share: the banner, drawing
//! a frame, rows, and cutting text to fit.

use super::setup::Style;
use anyhow::Result;
use std::io::Write;

const BANNER: [&str; 6] = [
    "██████╗  █████╗ ██████╗  █████╗  ██████╗ ███╗   ███╗ █████╗ ",
    "██╔══██╗██╔══██╗██╔══██╗██╔══██╗██╔════╝ ████╗ ████║██╔══██╗",
    "██████╔╝███████║██████╔╝███████║██║  ███╗██╔████╔██║███████║",
    "██╔═══╝ ██╔══██║██╔══██╗██╔══██║██║   ██║██║╚██╔╝██║██╔══██║",
    "██║     ██║  ██║██║  ██║██║  ██║╚██████╔╝██║ ╚═╝ ██║██║  ██║",
    "╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝ ╚═╝     ╚═╝╚═╝  ╚═╝",
];

/// The banner, with a line under it; one word when the terminal is narrow.
pub fn banner(out: &mut String, width: usize, st: &Style, under: &str) {
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
    out.push_str(&format!("  {}\n\n", st.dim(under)));
}

/// One row of a list: highlighted when chosen.
pub fn row(out: &mut String, st: &Style, chosen: bool, text: &str) {
    if chosen {
        out.push_str(&format!("  {} {}\n", st.accent("›"), st.bold(text)));
    } else {
        out.push_str(&format!("    {text}\n"));
    }
}

/// Draws over the last frame: each line is written over the old one and
/// cleared after, so the screen never goes blank between frames.
pub fn draw(frame: &str) -> Result<()> {
    let mut out = std::io::stdout().lock();
    // raw mode on Unix does not return the carriage at a line end
    let body = frame.replace('\n', "\x1b[K\r\n");
    write!(out, "\x1b[H{body}\x1b[K\x1b[J")?;
    out.flush()?;
    Ok(())
}

/// `s` cut or padded to `n` characters.
pub fn fit(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count > n {
        let cut: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        format!("{s}{}", " ".repeat(n - count))
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
