//! A note as it reads: headings, bold, code, links, to-dos and tables
//! drawn for the screen. The text stays Markdown; only the view changes.

use super::theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// Spans of a line of text: **bold**, *italic*, `code`, [[links]], [t](u).
pub fn inline(text: &str) -> Vec<Span<'static>> {
    let mut out: Vec<Span<'static>> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut Vec<Span<'static>>| {
        if !buf.is_empty() {
            out.push(Span::styled(std::mem::take(buf), theme::text()));
        }
    };
    let find = |from: usize, pat: &[char]| -> Option<usize> {
        (from..chars.len().saturating_sub(pat.len() - 1)).find(|&i| chars[i..].starts_with(pat))
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`'
            && let Some(e) = find(i + 1, &['`'])
        {
            flush(&mut buf, &mut out);
            out.push(Span::styled(chars[i + 1..e].iter().collect::<String>(), theme::code()));
            i = e + 1;
            continue;
        }
        if c == '*'
            && chars.get(i + 1) == Some(&'*')
            && let Some(e) = find(i + 2, &['*', '*'])
        {
            flush(&mut buf, &mut out);
            out.push(Span::styled(
                chars[i + 2..e].iter().collect::<String>(),
                theme::text().add_modifier(Modifier::BOLD),
            ));
            i = e + 2;
            continue;
        }
        if (c == '*' || c == '_')
            && chars.get(i + 1).is_some_and(|n| !n.is_whitespace())
            && let Some(e) = find(i + 1, &[c])
            && e > i + 1
        {
            flush(&mut buf, &mut out);
            out.push(Span::styled(
                chars[i + 1..e].iter().collect::<String>(),
                theme::text().add_modifier(Modifier::ITALIC),
            ));
            i = e + 1;
            continue;
        }
        if c == '['
            && chars.get(i + 1) == Some(&'[')
            && let Some(e) = find(i + 2, &[']', ']'])
        {
            flush(&mut buf, &mut out);
            out.push(Span::styled(chars[i + 2..e].iter().collect::<String>(), theme::link()));
            i = e + 2;
            continue;
        }
        if c == '['
            && let Some(close) = find(i + 1, &[']', '('])
            && let Some(end) = find(close + 2, &[')'])
        {
            flush(&mut buf, &mut out);
            out.push(Span::styled(chars[i + 1..close].iter().collect::<String>(), theme::link()));
            i = end + 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    flush(&mut buf, &mut out);
    out
}

/// The note's body as lines to show.
pub fn render(body: &str) -> Vec<Line<'static>> {
    let lines: Vec<&str> = body.lines().collect();
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut fenced = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        if t.starts_with("```") {
            fenced = !fenced;
            out.push(Line::from(Span::styled("  ─────", theme::dim())));
            i += 1;
            continue;
        }
        if fenced {
            out.push(Line::from(Span::styled(format!("  {line}"), theme::code())));
            i += 1;
            continue;
        }
        // a table: its rows lined up
        if t.starts_with('|') {
            let mut j = i;
            while j < lines.len() && lines[j].trim_start().starts_with('|') {
                j += 1;
            }
            let block = lines[i..j].join("\n");
            match crate::project::items::table(&block, None) {
                Some(table) => {
                    let rendered = crate::project::items::render(&table);
                    for (k, r) in rendered.iter().enumerate() {
                        let style = if k == 0 {
                            theme::text().add_modifier(Modifier::BOLD)
                        } else if k == 1 {
                            theme::dim()
                        } else {
                            theme::text()
                        };
                        let r = if k == 1 { r.replace('-', "─").replace('|', "┼") } else { r.replace('|', "│") };
                        out.push(Line::from(Span::styled(r, style)));
                    }
                }
                None => {
                    for l in &lines[i..j] {
                        out.push(Line::from(Span::styled(l.to_string(), theme::text())));
                    }
                }
            }
            i = j;
            continue;
        }
        let level = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&level) && t[level..].starts_with(' ') {
            let text = t[level..].trim().to_string();
            let style = match level {
                1 => theme::accent().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                2 => theme::accent().add_modifier(Modifier::BOLD),
                _ => theme::text().add_modifier(Modifier::BOLD),
            };
            if !out.is_empty() && level <= 2 {
                out.push(Line::from(""));
            }
            out.push(Line::from(Span::styled(text, style)));
            i += 1;
            continue;
        }
        let indent = " ".repeat(line.len() - t.len());
        let mut spans: Vec<Span<'static>> = vec![Span::raw(indent)];
        if let Some(rest) = t.strip_prefix("- [ ] ") {
            spans.push(Span::styled("☐ ", theme::accent()));
            spans.extend(inline(rest));
        } else if let Some(rest) = t.strip_prefix("- [x] ").or_else(|| t.strip_prefix("- [X] ")) {
            spans.push(Span::styled("☑ ", theme::ok()));
            spans.extend(inline(rest).into_iter().map(|s| s.style(theme::dim().add_modifier(Modifier::CROSSED_OUT))));
        } else if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            spans.push(Span::styled("• ", theme::accent()));
            spans.extend(inline(rest));
        } else if let Some(rest) = t.strip_prefix("> ") {
            spans.push(Span::styled("▍ ", theme::dim()));
            spans.extend(inline(rest).into_iter().map(|s| s.style(theme::dim())));
        } else {
            spans.extend(inline(t));
        }
        out.push(Line::from(spans));
        i += 1;
    }
    out
}

/// A line of the editor, colored lightly by what it is (the text stays as
/// it is written).
pub fn edit_style(line: &str, fenced: bool) -> Style {
    let t = line.trim_start();
    if fenced || t.starts_with("```") {
        return theme::code();
    }
    if t.starts_with('#') {
        return theme::accent().add_modifier(Modifier::BOLD);
    }
    if t.starts_with("- [x]") || t.starts_with("- [X]") {
        return theme::dim();
    }
    if t.starts_with('|') {
        return theme::text();
    }
    if t.starts_with('>') {
        return theme::dim();
    }
    theme::text()
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/ui/markdown.rs"]
mod tests;
