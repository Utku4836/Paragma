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
        let text = text.trim_start_matches('\u{feff}');
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

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/note/mod.rs"]
mod tests;
