//! The properties block: the YAML subset Obsidian writes for Properties.
//! Values are text or lists of text; the order of the keys is kept.
//!
//! ```text
//! title: "Refund window: 30 days"
//! tags: [payments, refunds]
//! files:
//!   - payments/refund.go
//! ```

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    List(Vec<String>),
}

#[derive(Clone, Debug, Default)]
pub struct Props {
    entries: Vec<(String, Value)>,
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
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(e) => e.1 = value,
            None => self.entries.push((key.to_string(), value)),
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
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, Value)> {
        self.entries.iter()
    }

    /// The block with its `---` fences, or "" when there are no properties.
    pub fn render(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }
        let mut out = String::from("---\n");
        for (k, v) in &self.entries {
            match v {
                Value::Text(s) => out.push_str(&format!("{k}: {}\n", quote(s))),
                Value::List(l) => {
                    let items: Vec<String> = l.iter().map(|s| quote_item(s)).collect();
                    out.push_str(&format!("{k}: [{}]\n", items.join(", ")));
                }
            }
        }
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

fn parse(block: &str) -> Props {
    let mut props = Props::default();
    let mut open_list: Option<String> = None;
    for line in block.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // "- item" lines under a key with no value: that key is a list
        if let (Some(key), Some(item)) = (&open_list, trimmed.strip_prefix("- ")) {
            if let Some(entry) = props.entries.iter_mut().find(|(k, _)| k == key) {
                match &mut entry.1 {
                    Value::List(l) => l.push(unquote(item)),
                    v => *v = Value::List(vec![unquote(item)]),
                }
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim().to_string(), value.trim());
        open_list = value.is_empty().then(|| key.clone());
        let value = match value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            Some(inner) => {
                Value::List(split_items(inner).iter().map(|s| unquote(s)).filter(|s| !s.is_empty()).collect())
            }
            None => Value::Text(unquote(value)),
        };
        props.entries.push((key, value));
    }
    props
}

/// Items of an inline list, commas inside quotes kept.
fn split_items(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match (quote, c) {
            (None, '"' | '\'') => {
                quote = Some(c);
                cur.push(c);
            }
            (Some(q), _) if c == q => {
                quote = None;
                cur.push(c);
            }
            (None, ',') => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        return s[1..s.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\");
    }
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        return s[1..s.len() - 1].replace("''", "'");
    }
    s.to_string()
}

fn needs_quotes(s: &str) -> bool {
    s.is_empty()
        || s.contains(": ")
        || s.contains(" #")
        || s.starts_with(|c: char| "[]{}&*!|>'\"%@`#-,?:".contains(c))
        || s != s.trim()
}

fn quote(s: &str) -> String {
    if needs_quotes(s) { format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")) } else { s.to_string() }
}

fn quote_item(s: &str) -> String {
    if needs_quotes(s) || s.contains(',') { quote_always(s) } else { s.to_string() }
}

fn quote_always(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
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
}
