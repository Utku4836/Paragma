//! What notes hold besides their text: to-dos (`- [ ] ...` lines), a due
//! date (the `due` property) and Markdown tables. Read and changed by their
//! form only, in any language: a checkbox, a date, a row of `|`.

use crate::output::{now, parse_date};

/// A to-do in a note: its number among the note's checkboxes (from 1,
/// done ones counted, so a number stays while others are closed), whether
/// done, its text.
#[derive(Clone, Debug, PartialEq)]
pub struct Todo {
    pub number: usize,
    pub done: bool,
    pub text: String,
}

/// The checkbox line's parts: (indent, done, text).
fn checkbox(line: &str) -> Option<(usize, bool, &str)> {
    let t = line.trim_start();
    let indent = line.len() - t.len();
    for (mark, done) in [("- [ ] ", false), ("* [ ] ", false), ("- [x] ", true), ("- [X] ", true), ("* [x] ", true)] {
        if let Some(rest) = t.strip_prefix(mark) {
            return Some((indent, done, rest));
        }
    }
    None
}

/// The to-dos of a note's body, outside code blocks.
pub fn todos(body: &str) -> Vec<Todo> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if let Some((_, done, text)) = checkbox(line) {
            out.push(Todo { number: out.len() + 1, done, text: text.trim().to_string() });
        }
    }
    out
}

/// Ticks off (or opens again) the to-do with this number: its text, or
/// None when there is no such to-do.
pub fn set_todo(body: &mut String, number: usize, done: bool) -> Option<String> {
    let mut seen = 0;
    let mut fenced = false;
    let mut text = None;
    let lines: Vec<String> = body
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                return line.to_string();
            }
            match checkbox(line) {
                Some((indent, _, t)) if !fenced => {
                    seen += 1;
                    if seen == number {
                        text = Some(t.trim().to_string());
                        return format!("{}- [{}] {t}", " ".repeat(indent), if done { "x" } else { " " });
                    }
                    line.to_string()
                }
                _ => line.to_string(),
            }
        })
        .collect();
    if text.is_some() {
        let ends = body.ends_with('\n');
        *body = lines.join("\n");
        if ends {
            body.push('\n');
        }
    }
    text
}

/// A due date as given: "2026-12-01", "+7d" (days from today), "+2w"
/// (weeks), or "none" to take it off (Some("")). None: not a date.
pub fn due_of(given: &str) -> Option<String> {
    let g = given.trim();
    if g.eq_ignore_ascii_case("none") || g == "-" {
        return Some(String::new());
    }
    if let Some(rest) = g.strip_prefix('+') {
        let (n, unit) = rest.split_at(rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len()));
        let n: i64 = n.parse().ok()?;
        let days = match unit {
            "" | "d" => n,
            "w" => n * 7,
            _ => return None,
        };
        return Some(crate::output::date(now() + days * 86_400));
    }
    parse_date(g)?;
    Some(g[..10].to_string())
}

/// Days from today to a due date (negative: past).
pub fn days_left(due: &str) -> Option<i64> {
    let at = parse_date(due)?;
    let today = parse_date(&crate::output::date(now()))?;
    Some((at - today) / 86_400)
}

/// "in 3 days", "today", "2 days ago".
pub fn when(days: i64) -> String {
    match days {
        0 => "today".into(),
        1 => "tomorrow".into(),
        -1 => "yesterday".into(),
        d if d > 0 => format!("in {d} days"),
        d => format!("{} days ago", -d),
    }
}

/// A Markdown table in a note: where its lines are, its header and rows.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    /// The first and the last line of it in the body.
    pub start: usize,
    pub end: usize,
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

fn cells(line: &str) -> Vec<String> {
    let t = line.trim().trim_start_matches('|').trim_end_matches('|');
    t.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_rule_line(line: &str) -> bool {
    let t = line.trim();
    t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// The first table of a body, or the first one under a heading
/// (`## Budget`) when one is named.
pub fn table(body: &str, under: Option<&str>) -> Option<Table> {
    let lines: Vec<&str> = body.lines().collect();
    let mut from = 0;
    if let Some(h) = under {
        from = lines
            .iter()
            .position(|l| l.trim_start_matches('#').trim().eq_ignore_ascii_case(h.trim()) && l.starts_with('#'))?;
    }
    let mut i = from;
    while i + 1 < lines.len() {
        if lines[i].trim_start().starts_with('|') && is_rule_line(lines[i + 1]) {
            let header = cells(lines[i]);
            let mut rows = Vec::new();
            let mut j = i + 2;
            while j < lines.len() && lines[j].trim_start().starts_with('|') {
                rows.push(cells(lines[j]));
                j += 1;
            }
            return Some(Table { start: i, end: j - 1, header, rows });
        }
        i += 1;
    }
    None
}

/// The table as lines, its columns lined up.
pub fn render(t: &Table) -> Vec<String> {
    let n = t.header.len().max(t.rows.iter().map(Vec::len).max().unwrap_or(0));
    let width = |k: usize| {
        std::iter::once(&t.header)
            .chain(t.rows.iter())
            .map(|r| r.get(k).map(|c| c.chars().count()).unwrap_or(0))
            .max()
            .unwrap_or(0)
            .max(3)
    };
    let widths: Vec<usize> = (0..n).map(width).collect();
    let line = |r: &Vec<String>| {
        let parts: Vec<String> = (0..n)
            .map(|k| {
                let c = r.get(k).map(String::as_str).unwrap_or("");
                format!("{c}{}", " ".repeat(widths[k] - c.chars().count()))
            })
            .collect();
        format!("| {} |", parts.join(" | "))
    };
    let mut out = vec![line(&t.header)];
    out.push(format!("|{}|", widths.iter().map(|w| "-".repeat(w + 2)).collect::<Vec<_>>().join("|")));
    out.extend(t.rows.iter().map(line));
    out
}

/// Puts a table back in place of the one it was read from.
pub fn replace(body: &str, old: &Table, new: &Table) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut out: Vec<String> = lines[..old.start].iter().map(|l| l.to_string()).collect();
    out.extend(render(new));
    out.extend(lines[old.end + 1..].iter().map(|l| l.to_string()));
    let mut s = out.join("\n");
    if body.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// A column by its name (any case) or its number (from 1).
pub fn column(t: &Table, which: &str) -> Option<usize> {
    let w = which.trim();
    if let Ok(n) = w.parse::<usize>() {
        return (n >= 1 && n <= t.header.len()).then_some(n - 1);
    }
    t.header.iter().position(|h| h.trim_matches('*').eq_ignore_ascii_case(w))
}

/// A cell as a number: "1.200,50", "1,200.50", "₺1200", "-3", "12%".
pub fn number(cell: &str) -> Option<f64> {
    let kept: String = cell.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-')).collect();
    if kept.is_empty() || !kept.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    // the last of '.' and ',' with 1-2 digits after it is the decimal mark
    let last = kept.rfind(['.', ',']);
    let normal = match last {
        Some(i) if kept.len() - i - 1 <= 2 => {
            let (int, frac) = kept.split_at(i);
            format!("{}.{}", int.replace(['.', ','], ""), &frac[1..])
        }
        _ => kept.replace(['.', ','], ""),
    };
    normal.parse().ok()
}

/// Whether a number cell writes its decimals after a comma ("1.200,50").
pub fn decimal_comma(cell: &str) -> bool {
    let kept: String = cell.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | ',')).collect();
    kept.rfind(',').is_some_and(|i| kept.len() - i - 1 <= 2 && kept.rfind('.').is_none_or(|d| d < i))
}

/// A row that is a total pg wrote: its first cell "**Total**".
pub fn is_total(row: &[String]) -> bool {
    row.first().is_some_and(|c| c == "**Total**")
}

/// A number as a cell: no ".0" for whole numbers.
pub fn cell_of(n: f64) -> String {
    if (n - n.round()).abs() < 1e-9 { format!("{}", n.round() as i64) } else { format!("{n:.2}") }
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/project/items.rs"]
mod tests;
