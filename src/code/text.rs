//! Symbols of the files read without a grammar: Markdown sections, the jobs
//! and services of YAML files (CI workflows, docker compose), what SQL
//! creates, the script blocks of Vue and Svelte files, HLSL's constant
//! buffers, and where Dockerfile stages end.

use super::parse::{Symbol, hash};
use regex::Regex;
use std::sync::LazyLock;

/// The sections of a Markdown file, by their headings: "Install.Windows"
/// for a "## Windows" under "# Install". Each runs to the next heading of
/// its level or above.
pub fn sections(src: &str) -> Vec<Symbol> {
    let lines: Vec<&str> = src.lines().collect();
    let mut heads: Vec<(usize, usize, String)> = Vec::new(); // line (from 0), level, text
    let mut fence: Option<char> = None;
    let mut i = 0;
    // front matter
    if lines.first().is_some_and(|l| l.trim() == "---") {
        i = lines.iter().skip(1).position(|l| l.trim() == "---").map(|p| p + 2).unwrap_or(0);
    }
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        let indent = line.len() - t.len();
        if indent <= 3 && (t.starts_with("```") || t.starts_with("~~~")) {
            let c = t.chars().next().unwrap_or('`');
            fence = match fence {
                Some(f) if f == c => None,
                None => Some(c),
                other => other,
            };
            i += 1;
            continue;
        }
        if fence.is_some() {
            i += 1;
            continue;
        }
        if indent <= 3 && t.starts_with('#') {
            let level = t.chars().take_while(|c| *c == '#').count();
            let rest = &t[level..];
            if level <= 6 && (rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')) {
                let text = clean_heading(rest.trim().trim_end_matches('#'));
                if !text.is_empty() {
                    heads.push((i, level, text));
                }
            }
        } else if i > 0 && indent <= 3 && !t.is_empty() && (t.chars().all(|c| c == '=') || t.chars().all(|c| c == '-'))
        {
            // a setext heading: the line above, underlined
            let above = lines[i - 1].trim();
            let is_text = !above.is_empty()
                && !above.starts_with(['-', '*', '+', '>', '|', '#'])
                && heads.last().is_none_or(|h| h.0 != i - 1);
            if is_text && (t.starts_with('=') || t.len() >= 2) {
                heads.push((i - 1, if t.starts_with('=') { 1 } else { 2 }, clean_heading(above)));
            }
        }
        i += 1;
    }
    let mut out = Vec::new();
    let mut chain: Vec<(usize, String)> = Vec::new(); // level, text
    for (n, (line, level, text)) in heads.iter().enumerate() {
        while chain.last().is_some_and(|c| c.0 >= *level) {
            chain.pop();
        }
        let end = heads[n + 1..].iter().find(|h| h.1 <= *level).map(|h| h.0).unwrap_or(lines.len());
        // the section's last line with text
        let mut last = end.max(line + 1);
        while last > line + 1 && lines[last - 1].trim().is_empty() {
            last -= 1;
        }
        let qualified =
            chain.iter().map(|c| c.1.as_str()).chain(std::iter::once(text.as_str())).collect::<Vec<_>>().join(".");
        let body = lines[line + 1..last].join("\n");
        out.push(Symbol {
            name: text.clone(),
            qualified,
            kind: "section",
            start: line + 1,
            end: last,
            signature: lines[*line].trim().chars().take(120).collect(),
            doc: first_text(&lines[line + 1..last]),
            calls: Vec::new(),
            about: prose(&lines[line + 1..last], 600),
            body_hash: hash(&body),
        });
        chain.push((*level, text.clone()));
    }
    out
}

/// A heading's words: without code marks, emphasis and link targets.
fn clean_heading(h: &str) -> String {
    let mut out = String::new();
    let mut chars = h.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' | '*' => {}
            ']' if chars.peek() == Some(&'(') => {
                for c in chars.by_ref() {
                    if c == ')' {
                        break;
                    }
                }
            }
            '[' => {}
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(100).collect()
}

/// The section's prose, up to `max` bytes: its words without code blocks
/// and markup, for its meaning.
fn prose(lines: &[&str], max: usize) -> String {
    let mut out = String::new();
    let mut fence = false;
    for l in lines {
        let t = l.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence || t.is_empty() || t.starts_with('#') || t.starts_with("<!--") {
            continue;
        }
        out.push_str(&clean_heading(t.trim_start_matches(['-', '*', '>', '|', ' '])));
        out.push(' ');
        if out.len() >= max {
            break;
        }
    }
    let mut end = out.len().min(max);
    while !out.is_char_boundary(end) {
        end -= 1;
    }
    out[..end].trim().to_string()
}

/// The first line of prose in a section: what it is about.
fn first_text(lines: &[&str]) -> String {
    let mut fence = false;
    for l in lines {
        let t = l.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence || t.is_empty() || t.starts_with('#') || t.starts_with('|') || t.starts_with("<!--") {
            continue;
        }
        return clean_heading(t.trim_start_matches(['-', '*', '>', ' '])).chars().take(160).collect();
    }
    String::new()
}

/// The jobs of a CI workflow (GitHub Actions `jobs:`, GitLab CI's top
/// level jobs) and the services of a docker compose file.
pub fn yaml_jobs(src: &str) -> Vec<Symbol> {
    let lines: Vec<&str> = src.lines().collect();
    // lines that matter: (line, indent, key or None)
    let rows: Vec<(usize, usize, Option<&str>)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            let t = l.trim_start();
            if t.is_empty() || t.starts_with('#') || *l == "---" {
                return None;
            }
            let indent = l.len() - t.len();
            Some((i, indent, key_of(t)))
        })
        .collect();
    let top: Vec<usize> = (0..rows.len()).filter(|&r| rows[r].1 == 0).collect();
    let block = |r: usize| -> (usize, usize) {
        // the rows below a key, up to the next row as little indented
        let ind = rows[r].1;
        let end = (r + 1..rows.len()).find(|&x| rows[x].1 <= ind).unwrap_or(rows.len());
        (r + 1, end)
    };
    let mut out = Vec::new();
    let mut found_keys = false;
    for &r in &top {
        let (kind, key) = match rows[r].2 {
            Some("jobs") => ("job", "jobs"),
            Some("services") => ("service", "services"),
            _ => continue,
        };
        found_keys = true;
        let (a, b) = block(r);
        let Some(child) = rows.get(a).filter(|_| a < b).map(|x| x.1) else { continue };
        for x in a..b {
            if rows[x].1 == child
                && let Some(name) = rows[x].2
            {
                let (_, end) = block(x);
                out.push(yaml_symbol(&lines, &rows, x, end, kind, name, key));
            }
        }
    }
    if !found_keys {
        // GitLab CI: top level keys whose block runs a script
        const RESERVED: &[&str] = &[
            "stages",
            "variables",
            "default",
            "include",
            "workflow",
            "image",
            "before_script",
            "after_script",
            "cache",
        ];
        for &r in &top {
            let Some(name) = rows[r].2.filter(|n| !n.starts_with('.') && !RESERVED.contains(n)) else { continue };
            let (a, b) = block(r);
            let child = rows.get(a).map(|x| x.1).unwrap_or(0);
            let runs =
                (a..b).any(|x| rows[x].1 == child && matches!(rows[x].2, Some("script" | "trigger" | "extends")));
            if runs {
                out.push(yaml_symbol(&lines, &rows, r, b, "job", name, ""));
            }
        }
    }
    out
}

fn key_of(t: &str) -> Option<&str> {
    let t = t.strip_prefix("- ").unwrap_or(t);
    let (k, rest) = t.split_once(':')?;
    let k = k.trim().trim_matches(['"', '\'']);
    if k.is_empty() || k.contains(' ') || !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    Some(k)
}

fn yaml_symbol(
    lines: &[&str],
    rows: &[(usize, usize, Option<&str>)],
    r: usize,
    end: usize,
    kind: &'static str,
    name: &str,
    parent: &str,
) -> Symbol {
    let start = rows[r].0;
    let last = if end < rows.len() { rows[end - 1].0 } else { rows[rows.len() - 1].0 }.max(start);
    // its `name:` (a GitHub job's title), else the comment above it
    let child = rows.get(r + 1).map(|x| x.1).unwrap_or(0);
    let title = rows[r + 1..end]
        .iter()
        .find(|x| x.1 == child && x.2 == Some("name"))
        .and_then(|x| lines[x.0].split_once(':').map(|(_, v)| v.trim().trim_matches(['"', '\'']).to_string()));
    let above = start.checked_sub(1).map(|i| lines[i].trim()).filter(|l| l.starts_with('#'));
    let doc = title.or_else(|| above.map(|l| l.trim_start_matches('#').trim().to_string())).unwrap_or_default();
    let body = lines[start..=last].join("\n");
    // what it needs or depends on
    let mut calls = Vec::new();
    for x in &rows[r + 1..end] {
        if x.1 == child && matches!(x.2, Some("needs" | "depends_on" | "extends")) {
            let v = lines[x.0].split_once(':').map(|(_, v)| v).unwrap_or("");
            calls.extend(
                v.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.'))
                    .filter(|w| !w.is_empty())
                    .map(str::to_string),
            );
        }
    }
    Symbol {
        name: name.to_string(),
        qualified: if parent.is_empty() { name.to_string() } else { format!("{parent}.{name}") },
        kind,
        start: start + 1,
        end: last + 1,
        signature: lines[start].trim().chars().take(120).collect(),
        doc: doc.chars().take(160).collect(),
        calls,
        body_hash: hash(&body.replacen(name, "", 1)),
        about: String::new(),
    }
}

/// A Vue or Svelte file with everything but its <script> blocks blanked
/// (lines stay where they are), and whether a block is TypeScript.
pub fn scripts_only(src: &str) -> (bool, String) {
    let lower = src.to_ascii_lowercase();
    let mut keep = vec![false; src.len()];
    let mut ts = false;
    let mut at = 0;
    while let Some(open) = lower[at..].find("<script").map(|p| p + at) {
        let Some(tag_end) = lower[open..].find('>').map(|p| p + open + 1) else { break };
        let tag = &lower[open..tag_end];
        if tag.contains("lang=\"ts\"") || tag.contains("lang='ts'") || tag.contains("typescript") {
            ts = true;
        }
        let close = lower[tag_end..].find("</script").map(|p| p + tag_end).unwrap_or(src.len());
        keep[tag_end..close].iter_mut().for_each(|k| *k = true);
        at = close.max(tag_end);
    }
    let out: Vec<u8> = src.bytes().zip(keep).map(|(b, k)| if k || b == b'\n' { b } else { b' ' }).collect();
    (ts, String::from_utf8(out).unwrap_or_default())
}

/// A Dockerfile stage runs to the next FROM; it uses the stages it copies from.
pub fn stages_end(out: &mut [Symbol], src: &str) {
    let lines: Vec<&str> = src.lines().collect();
    let mut stages: Vec<usize> = (0..out.len()).filter(|&i| out[i].kind == "stage").collect();
    stages.sort_by_key(|&i| out[i].start);
    for (n, &i) in stages.iter().enumerate() {
        let next = stages.get(n + 1).map(|&j| out[j].start - 1).unwrap_or(lines.len());
        let mut end = next.max(out[i].start);
        while end > out[i].start && lines.get(end - 1).is_some_and(|l| l.trim().is_empty()) {
            end -= 1;
        }
        out[i].end = end;
        let mut uses = Vec::new();
        for l in &lines[out[i].start.saturating_sub(1)..end] {
            if let Some(from) = l.split_whitespace().find_map(|w| w.strip_prefix("--from=")) {
                uses.push(from.to_string());
            }
        }
        out[i].calls = uses;
    }
}

static SQL_CREATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\bcreate\s+(?:or\s+replace\s+)?(?:(?:temp|temporary|unique|materialized|virtual|recursive)\s+)*(table|view|index|function|procedure|trigger|type|schema|sequence)\s+(?:if\s+not\s+exists\s+)?((?:[\w$]+|"[^"]+"|`[^`]+`|\[[^\]]+\])(?:\.(?:[\w$]+|"[^"]+"|`[^`]+`|\[[^\]]+\]))*)"#,
    )
    .expect("pattern")
});

/// What a SQL file creates: tables, views, indexes, functions, triggers,
/// types. Each runs to the `;` that ends its statement.
pub fn sql_objects(src: &str) -> Vec<Symbol> {
    let code = sql_code_only(src);
    let mut out = Vec::new();
    let line_of = |byte: usize| src[..byte].matches('\n').count() + 1;
    for m in SQL_CREATE.captures_iter(&code) {
        let (Some(all), Some(what), Some(name)) = (m.get(0), m.get(1), m.get(2)) else { continue };
        let end = code[all.end()..].find(';').map(|p| p + all.end()).unwrap_or(code.len());
        let name = name.as_str().replace(['"', '`', '[', ']'], "");
        let short = name.rsplit('.').next().unwrap_or(&name).to_string();
        let kind = match what.as_str().to_ascii_lowercase().as_str() {
            "table" => "table",
            "view" => "view",
            "index" => "index",
            "function" | "procedure" => "function",
            "trigger" => "trigger",
            "schema" => "module",
            _ => "type",
        };
        let text = &src[all.start()..end.min(src.len())];
        out.push(Symbol {
            qualified: name.clone(),
            kind,
            start: line_of(all.start()),
            end: line_of(end.min(src.len())),
            signature: text.lines().next().unwrap_or("").trim().chars().take(120).collect(),
            doc: comment_above(src, all.start()),
            calls: Vec::new(),
            body_hash: hash(&text.replacen(&short, "", 1)),
            about: String::new(),
            name: short,
        });
    }
    out
}

/// SQL with its comments, strings and dollar-quoted bodies blanked (lines
/// and bytes stay where they are), so a ';' or a CREATE in them counts not.
fn sql_code_only(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = b.to_vec();
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for x in out.iter_mut().take(to).skip(from) {
            if *x != b'\n' {
                *x = b' ';
            }
        }
    };
    let mut i = 0;
    while i < b.len() {
        if !src.is_char_boundary(i) {
            i += 1; // inside a character: what is looked for here is ASCII
            continue;
        }
        let rest = &src[i..];
        let end = if rest.starts_with("--") {
            rest.find('\n').map(|p| i + p).unwrap_or(b.len())
        } else if let Some(body) = rest.strip_prefix("/*") {
            body.find("*/").map(|p| i + p + 4).unwrap_or(b.len())
        } else if b[i] == b'\'' {
            rest[1..].find('\'').map(|p| i + p + 2).unwrap_or(b.len())
        } else if b[i] == b'$' {
            // $$ ... $$ or $tag$ ... $tag$: a function's body, kept apart
            match rest[1..].find('$').map(|p| &rest[..p + 2]) {
                Some(tag) if tag[1..tag.len() - 1].chars().all(|c| c.is_alphanumeric() || c == '_') => {
                    rest[tag.len()..].find(tag).map(|p| i + tag.len() + p + tag.len()).unwrap_or(b.len())
                }
                _ => i + 1,
            }
        } else {
            i += 1;
            continue;
        };
        if end > i + 1 || !rest.starts_with('$') {
            blank(&mut out, i, end);
        }
        i = end.max(i + 1);
    }
    String::from_utf8(out).unwrap_or_default()
}

/// The `--` or `//` comment on the line above a byte.
fn comment_above(src: &str, at: usize) -> String {
    let before = &src[..at];
    let mut lines = before.lines().rev();
    if !before.ends_with('\n') {
        lines.next(); // the line `at` is on
    }
    lines
        .next()
        .map(str::trim)
        .and_then(|l| l.strip_prefix("--").or_else(|| l.strip_prefix("//")))
        .map(|l| l.trim().chars().take(160).collect())
        .unwrap_or_default()
}

static HLSL_BUFFER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*(cbuffer|tbuffer)[ \t]+(\w+)").expect("pattern"));

/// HLSL's constant buffers (cbuffer Params : register(b0) { ... }), which
/// the C++ grammar it is read with does not know.
pub fn hlsl_buffers(src: &str) -> Vec<Symbol> {
    let mut out = Vec::new();
    for m in HLSL_BUFFER.captures_iter(src) {
        let (Some(all), Some(name)) = (m.get(0), m.get(2)) else { continue };
        // to the brace that closes its block
        let open = src[all.end()..].find('{').map(|p| p + all.end());
        let mut end = src.len();
        if let Some(o) = open {
            let mut depth = 0;
            for (k, c) in src[o..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = o + k + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        let line_of = |byte: usize| src[..byte].matches('\n').count() + 1;
        let text = &src[all.start()..end];
        out.push(Symbol {
            name: name.as_str().to_string(),
            qualified: name.as_str().to_string(),
            kind: "binding",
            start: line_of(all.start()),
            end: line_of(end),
            signature: text.lines().next().unwrap_or("").trim().chars().take(120).collect(),
            doc: comment_above(src, all.start()),
            calls: Vec::new(),
            body_hash: hash(&text.replacen(name.as_str(), "", 1)),
            about: String::new(),
        });
    }
    out
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/code/text.rs"]
mod tests;
