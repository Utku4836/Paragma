//! The properties block: the YAML Obsidian writes for Properties. Text and
//! lists of text are read as values; anything else (a nested map, a list
//! of maps, multi-line text) is kept as it is written. The order of the
//! keys is kept, and a key pg does not change is written back byte for
//! byte when the note is rewritten: what pg does not understand, it does
//! not break.
//!
//! ```text
//! title: "Refund window: 30 days"
//! tags: [payments, refunds]
//! files:
//!   - payments/refund.go
//! relationships:          (kept as written)
//!   - target: "[[design]]"
//!     type: depends_on
//! ```

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    List(Vec<String>),
    /// A value pg does not read: the YAML after `key:`, as written.
    Raw(String),
}

#[derive(Clone, Debug, Default)]
pub struct Props {
    entries: Vec<(String, Value)>,
    /// Each key's lines as they were read (comment lines before it too):
    /// written back as they were while the key is not changed.
    original: HashMap<String, String>,
    /// Comment lines after the last key.
    tail: String,
}

impl Props {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Text(s) if !s.is_empty() => Some(s),
            _ => None,
        }
    }

    pub fn set(&mut self, key: &str, value: Value) {
        if self.get(key) == Some(&value) {
            return; // the same: written as it was
        }
        self.original.remove(key);
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(e) => e.1 = value,
            None => self.entries.push((key.to_string(), value)),
        }
    }

    /// Sets a key and puts it first (an id the note did not have).
    pub fn set_first(&mut self, key: &str, value: Value) {
        self.set(key, value);
        if let Some(i) = self.entries.iter().position(|(k, _)| k == key) {
            let e = self.entries.remove(i);
            self.entries.insert(0, e);
        }
    }

    pub fn set_text(&mut self, key: &str, value: impl Into<String>) {
        self.set(key, Value::Text(value.into()));
    }

    /// A list property's items (a text property as one item).
    pub fn list(&self, key: &str) -> Vec<String> {
        match self.get(key) {
            Some(Value::List(l)) => l.clone(),
            Some(Value::Text(s)) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        }
    }

    pub fn remove(&mut self, key: &str) {
        self.entries.retain(|(k, _)| k != key);
        self.original.remove(key);
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, Value)> {
        self.entries.iter()
    }

    /// The block with its `---` fences, or "" when there are no properties.
    pub fn render(&self) -> String {
        if self.entries.is_empty() && self.tail.is_empty() {
            return String::new();
        }
        let mut out = String::from("---\n");
        for (k, v) in &self.entries {
            if let Some(o) = self.original.get(k) {
                out.push_str(o);
                continue;
            }
            match v {
                Value::Text(s) => out.push_str(&format!("{k}: {}\n", quote(s))),
                Value::List(l) => {
                    let items: Vec<String> = l.iter().map(|s| quote_item(s)).collect();
                    out.push_str(&format!("{k}: [{}]\n", items.join(", ")));
                }
                Value::Raw(r) => {
                    out.push_str(&format!("{k}:{r}"));
                    if !r.ends_with('\n') {
                        out.push('\n');
                    }
                }
            }
        }
        out.push_str(&self.tail);
        out.push_str("---\n");
        out
    }
}

/// Splits a file into its properties and its body.
pub fn split(text: &str) -> (Props, &str) {
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return (Props::default(), text);
    };
    let mut end = None;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            end = Some((offset, offset + line.len()));
            break;
        }
        offset += line.len();
    }
    let Some((block_end, body_start)) = end else {
        return (Props::default(), text);
    };
    (parse(&rest[..block_end]), &rest[body_start..])
}

/// A key's line: at the start of the line, not a list item or a comment.
fn key_line(line: &str) -> Option<(&str, &str)> {
    if line.starts_with([' ', '\t', '-', '#']) || line.trim().is_empty() {
        return None;
    }
    let (k, v) = line.split_once(':')?;
    let k = k.trim();
    (!k.is_empty() && (v.is_empty() || v.starts_with([' ', '\t', '\r', '\n']))).then_some((k, v))
}

fn parse(block: &str) -> Props {
    let mut props = Props::default();
    let lines: Vec<&str> = block.split_inclusive('\n').collect();
    let mut prefix = String::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let Some((key, value)) = key_line(line.trim_end_matches(['\r', '\n'])) else {
            // a comment or a stray line before a key: kept with that key
            prefix.push_str(line);
            i += 1;
            continue;
        };
        // the lines that belong to it: indented, list items, blank ones
        // inside it
        let mut j = i + 1;
        while j < lines.len() && key_line(lines[j].trim_end_matches(['\r', '\n'])).is_none() {
            let t = lines[j];
            let more = t.starts_with([' ', '\t', '-']);
            let blank_inside = t.trim().is_empty()
                && lines[j + 1..]
                    .iter()
                    .find(|l| !l.trim().is_empty())
                    .is_some_and(|l| l.starts_with([' ', '\t', '-']));
            if !(more || blank_inside) {
                break;
            }
            j += 1;
        }
        let rest: Vec<&str> = lines[i + 1..j].iter().map(|l| l.trim_end_matches(['\r', '\n'])).collect();
        let mut raw_after = value.to_string();
        if !raw_after.ends_with('\n') {
            raw_after.push('\n');
        }
        for l in &lines[i + 1..j] {
            raw_after.push_str(l);
        }
        let value = read_value(value.trim(), &rest).unwrap_or(Value::Raw(raw_after));
        let mut original = std::mem::take(&mut prefix);
        for l in &lines[i..j] {
            original.push_str(l);
        }
        if !original.ends_with('\n') {
            original.push('\n');
        }
        let key = key.to_string();
        props.original.insert(key.clone(), original);
        props.entries.push((key, value));
        i = j;
    }
    props.tail = prefix;
    props
}

/// A value pg reads: text, an inline list, or a block of simple list
/// items. None for anything else (kept as written).
fn read_value(value: &str, rest: &[&str]) -> Option<Value> {
    let items: Vec<&str> = rest.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if value.is_empty() {
        if items.is_empty() {
            return Some(Value::Text(String::new()));
        }
        // "- item" lines, all at one indent, each a plain value
        let indent = |l: &str| l.len() - l.trim_start().len();
        let first = rest.iter().find(|l| !l.trim().is_empty()).map(|l| indent(l))?;
        let mut out = Vec::new();
        for l in rest.iter().filter(|l| !l.trim().is_empty()) {
            let item = l.trim().strip_prefix("- ").or_else(|| (l.trim() == "-").then_some(""))?;
            if indent(l) != first || !simple(item) {
                return None;
            }
            out.push(unquote(item));
        }
        return Some(Value::List(out));
    }
    if !items.is_empty() {
        return None; // text over several lines
    }
    if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        let parts = split_items(inner)?;
        return Some(Value::List(parts.iter().map(|s| unquote(s)).filter(|s| !s.is_empty()).collect()));
    }
    simple(value).then(|| Value::Text(unquote(value)))
}

/// A value pg reads as text: not a map, a nested list, a block scalar or
/// an unclosed quote.
fn simple(v: &str) -> bool {
    let v = v.trim();
    if v.starts_with(['{', '[', '|', '>', '&', '*', '!']) {
        return false;
    }
    for q in ['"', '\''] {
        if v.starts_with(q) {
            return v.len() >= 2 && v.ends_with(q);
        }
    }
    // "a: b" unquoted in an item is a map
    !v.contains(": ")
}

/// Items of an inline list, commas inside quotes kept; None for a list
/// holding lists or maps.
fn split_items(s: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    // in double quotes a backslash escapes the next character: \" ends nothing
    let mut escaped = false;
    for c in s.chars() {
        if escaped {
            escaped = false;
            cur.push(c);
            continue;
        }
        match (quote, c) {
            (Some('"'), '\\') => {
                escaped = true;
                cur.push(c);
            }
            (None, '"' | '\'') => {
                quote = Some(c);
                cur.push(c);
            }
            (Some(q), _) if c == q => {
                quote = None;
                cur.push(c);
            }
            (None, '[' | ']' | '{' | '}') => return None,
            (None, ',') => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    Some(out)
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        return unescape(&s[1..s.len() - 1]);
    }
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        return s[1..s.len() - 1].replace("''", "'");
    }
    s.to_string()
}

/// The text of a double-quoted YAML string: the escapes `quote_always`
/// writes read back as what they stand for; others kept as they are.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Text that YAML would read otherwise than as this text, unquoted.
fn needs_quotes(s: &str) -> bool {
    s.is_empty()
        || s.contains(": ")
        || s.contains(" #")
        || s.ends_with(':')
        || s.contains(['\n', '\r', '\t'])
        || s.starts_with(|c: char| "[]{}&*!|>'\"%@`#-,?:".contains(c))
        || s != s.trim()
}

fn quote(s: &str) -> String {
    if needs_quotes(s) { quote_always(s) } else { s.to_string() }
}

/// An item of an inline list: also quoted when it holds what ends or
/// nests an item there.
fn quote_item(s: &str) -> String {
    // a quote inside an item would start a quoted one when read
    if needs_quotes(s) || s.contains([',', '[', ']', '{', '}', '#', ':', '"', '\'']) {
        quote_always(s)
    } else {
        s.to_string()
    }
}

fn quote_always(s: &str) -> String {
    let escaped =
        s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_lists_and_quotes() {
        let (p, body) =
            split("---\ntitle: \"Refund: 30 days\"\nfiles:\n  - a.go\n  - b.go\ntags: []\nnote:\n---\nbody\n");
        assert_eq!(p.text("title"), Some("Refund: 30 days"));
        assert_eq!(p.get("files"), Some(&Value::List(vec!["a.go".into(), "b.go".into()])));
        assert_eq!(p.get("tags"), Some(&Value::List(vec![])));
        assert_eq!(p.text("note"), None);
        assert_eq!(body, "body\n");
    }

    #[test]
    fn quoting_round_trips() {
        let mut p = Props::default();
        p.set_text("title", "Refund: 30 days");
        p.set("files", Value::List(vec!["a, b.go".into(), "c.go".into()]));
        let (q, _) = split(&p.render());
        assert_eq!(q.text("title"), Some("Refund: 30 days"));
        assert_eq!(q.get("files"), Some(&Value::List(vec!["a, b.go".into(), "c.go".into()])));
    }

    #[test]
    fn no_block_means_no_props() {
        let (p, body) = split("just text\n---\nmore\n");
        assert_eq!(p.iter().count(), 0);
        assert_eq!(body, "just text\n---\nmore\n");
    }

    #[test]
    fn what_pg_does_not_read_is_written_back_as_it_was() {
        let block = "---\n# from Obsidian\ntitle: Intro\nrelationships:\n  - target: \"[[design]]\"\n    type: depends_on\n  - target: \"[[roadmap]]\"\n    type: related\nprovenance:\n  source: chat\n  confidence: 0.8\nsummary: |\n  Two lines\n\n  of text.\ntags:\n- engine\n- \"a]b\"\nsources: [https://example.com/a?b=1&c=2, \"plain\"]\nnested: [[a, b], c]\n---\nbody\n";
        let (mut p, body) = split(block);
        assert_eq!(body, "body\n");
        assert!(matches!(p.get("relationships"), Some(Value::Raw(_))));
        assert!(matches!(p.get("provenance"), Some(Value::Raw(_))));
        assert!(matches!(p.get("summary"), Some(Value::Raw(_))));
        assert!(matches!(p.get("nested"), Some(Value::Raw(_))));
        assert_eq!(p.list("tags"), vec!["engine", "a]b"]);
        assert_eq!(p.list("sources"), vec!["https://example.com/a?b=1&c=2", "plain"]);
        // untouched: the same bytes
        assert_eq!(p.render(), block.trim_end_matches("body\n"));
        // pg adds an id and changes a list: the rest stays as written
        p.set_text("id", "n1");
        p.set("tags", Value::List(vec!["engine".into(), "a]b".into(), "x, y".into()]));
        let out = p.render();
        assert!(out.contains("relationships:\n  - target: \"[[design]]\"\n    type: depends_on\n"), "{out}");
        assert!(out.contains("summary: |\n  Two lines\n\n  of text.\n"), "{out}");
        assert!(out.contains("tags: [engine, \"a]b\", \"x, y\"]\n"), "{out}");
        assert!(out.contains("id: n1\n---\n"), "{out}");
        let (again, _) = split(&format!("{out}body\n"));
        assert_eq!(again.list("tags"), vec!["engine", "a]b", "x, y"]);
        assert_eq!(again.get("provenance"), p.get("provenance"));
    }

    #[test]
    fn items_and_text_are_written_as_valid_yaml() {
        for (item, quoted) in [("a]b", true), ("x#y", true), ("k:v", true), ("{a}", true), ("plain", false)] {
            assert_eq!(quote_item(item).starts_with('"'), quoted, "{item}");
        }
        assert!(quote("Note:").starts_with('"'));
        assert_eq!(quote("two\nlines"), "\"two\\nlines\"");
    }
}
