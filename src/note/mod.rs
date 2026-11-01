//! A note: one Markdown file with properties on top (the YAML block
//! Obsidian shows as Properties) and a Markdown body.
//!
//! ```text
//! ---
//! id: e45
//! type: error
//! files: [catalog/catalog.go]
//! ---
//! ## Fix
//! Normalise line endings before hashing.
//! ```

mod chunk;
mod props;

pub use chunk::{Chunk, chunks, rule_line};
pub use props::{Props, Value};

/// What a note is; the `type` property says it (not the folder it is in).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rule,
    Decision,
    Error,
    Project,
    Note,
}

impl Kind {
    pub fn parse(s: &str) -> Kind {
        match s.trim().to_ascii_lowercase().as_str() {
            "rule" | "rules" => Kind::Rule,
            "decision" => Kind::Decision,
            "error" => Kind::Error,
            "project" => Kind::Project,
            _ => Kind::Note,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Rule => "rule",
            Kind::Decision => "decision",
            Kind::Error => "error",
            Kind::Project => "project",
            Kind::Note => "note",
        }
    }

    /// The letter of this kind's ids: r12, d7, e45, p1, n88.
    pub fn id_prefix(self) -> char {
        self.as_str().chars().next().unwrap_or('n')
    }
}

#[derive(Clone, Debug, Default)]
pub struct Note {
    pub props: Props,
    pub body: String,
}

impl Note {
    pub fn parse(text: &str) -> Note {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let (props, body) = props::split(text);
        Note { props, body: body.to_string() }
    }

    pub fn render(&self) -> String {
        let mut out = self.props.render();
        out.push_str(&self.body);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    }

    pub fn kind(&self) -> Kind {
        Kind::parse(self.props.text("type").unwrap_or("note"))
    }

    pub fn id(&self) -> Option<&str> {
        self.props.text("id")
    }

    /// The text of the "## name" section, without its heading.
    pub fn section(&self, name: &str) -> Option<String> {
        let mut lines = self.body.lines().skip_while(|l| !is_heading(l, name));
        lines.next()?;
        let text: Vec<&str> = lines.take_while(|l| !l.starts_with("## ")).collect();
        Some(text.join("\n").trim().to_string())
    }

    /// Replaces the "## name" section's text, adding the section at the end
    /// when the note has none.
    pub fn set_section(&mut self, name: &str, text: &str) {
        let text = text.trim();
        let mut out = String::new();
        let mut lines = self.body.lines().peekable();
        let mut found = false;
        while let Some(l) = lines.next() {
            if !found && is_heading(l, name) {
                found = true;
                out.push_str(&format!("{l}\n{text}\n")); // the heading as it was written
                while lines.peek().is_some_and(|l| !l.starts_with("## ")) {
                    lines.next();
                }
                if lines.peek().is_some() {
                    out.push('\n');
                }
            } else {
                out.push_str(l);
                out.push('\n');
            }
        }
        if !found {
            let kept = out.trim_end();
            out = if kept.is_empty() { String::new() } else { format!("{kept}\n\n") };
            out.push_str(&format!("## {name}\n{text}\n"));
        }
        self.body = out;
    }

    /// The title: the `title` property, else the first heading, else "".
    pub fn title(&self) -> String {
        if let Some(t) = self.props.text("title") {
            return t.to_string();
        }
        self.body.lines().find_map(|l| l.strip_prefix("# ")).map(|t| t.trim().to_string()).unwrap_or_default()
    }
}

fn is_heading(line: &str, name: &str) -> bool {
    line.strip_prefix("## ").is_some_and(|h| h.trim().eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_are_read_and_replaced() {
        let mut n = Note::parse("## Goal\nShip it.\n\n## Steps\n- [ ] a\n");
        assert_eq!(n.section("goal").as_deref(), Some("Ship it."));
        n.set_section("Goal", "Ship it today.");
        n.set_section("Handoffs", "### x\ndone");
        assert_eq!(n.body, "## Goal\nShip it today.\n\n## Steps\n- [ ] a\n\n## Handoffs\n### x\ndone\n");
        assert_eq!(n.section("Steps").as_deref(), Some("- [ ] a"));
        assert_eq!(n.section("Nothing"), None);
    }

    const ERROR: &str = "---\nid: e45\ntype: error\nfiles: [catalog/catalog.go, .gitattributes]\nseen: 3\n---\n## Symptom\nTests fail.\n\n## Fix\nNormalise line endings.\n";

    #[test]
    fn parses_props_and_sections() {
        let n = Note::parse(ERROR);
        assert_eq!(n.id(), Some("e45"));
        assert_eq!(n.kind(), Kind::Error);
        assert_eq!(
            n.props.get("files"),
            Some(&Value::List(vec!["catalog/catalog.go".into(), ".gitattributes".into()]))
        );
        assert_eq!(n.props.text("seen"), Some("3"));
    }

    #[test]
    fn renders_what_it_parsed() {
        assert_eq!(Note::parse(ERROR).render(), ERROR);
    }

    #[test]
    fn a_file_without_props_is_a_plain_note() {
        let n = Note::parse("# Shader notes\nUse half floats.\n");
        assert_eq!(n.kind(), Kind::Note);
        assert_eq!(n.title(), "Shader notes");
        assert_eq!(n.render(), "# Shader notes\nUse half floats.\n");
    }
}
