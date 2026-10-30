//! A note cut into the pieces search returns: a short note whole, a long
//! one by section, a rules note by rule.

use super::{Kind, Note};

/// Notes shorter than this stay one piece.
const WHOLE_NOTE: usize = 1500;

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    /// What the piece is called in output: the note id, or the rule id for a rule.
    pub id: String,
    pub kind: Kind,
    /// The section (or rule group) it comes from; "" for a whole note.
    pub heading: String,
    pub text: String,
}

pub fn chunks(note: &Note, fallback_id: &str) -> Vec<Chunk> {
    let id = note.id().unwrap_or(fallback_id).to_string();
    let kind = note.kind();
    let mut out = Vec::new();
    let mut push = |id: &str, kind: Kind, heading: &str, text: &str| {
        let text = text.trim();
        if !text.is_empty() {
            out.push(Chunk { id: id.to_string(), kind, heading: heading.to_string(), text: text.to_string() });
        }
    };
    if kind == Kind::Rule {
        // "- r1: text" lines under "## Group" headings
        let mut group = String::new();
        for line in note.body.lines() {
            if let Some(h) = line.strip_prefix("## ") {
                group = h.trim().to_string();
            } else if let Some((rid, text)) = rule_line(line) {
                push(rid, Kind::Rule, &group, text);
            }
        }
        return out;
    }
    if note.body.len() < WHOLE_NOTE {
        push(&id, kind, "", &note.body);
        return out;
    }
    let mut heading = String::new();
    let mut text = String::new();
    for line in note.body.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            push(&id, kind, &heading, &text);
            heading = h.trim().to_string();
            text.clear();
        } else {
            text.push_str(line);
            text.push('\n');
        }
    }
    push(&id, kind, &heading, &text);
    out
}

/// "- r12: Amounts are int64" -> ("r12", "Amounts are int64").
pub fn rule_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start().strip_prefix("- ")?;
    let (id, text) = rest.split_once(": ")?;
    let digits = id.strip_prefix('r')?;
    (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())).then_some((id, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_are_one_piece_each() {
        let n = Note::parse(
            "---\ntype: rules\n---\n## Money\n- r1: Amounts are int64.\n- r2: Round half up.\n\n## Time\n- r3: Use clock.Now.\n- not a rule\n",
        );
        let c = chunks(&n, "rules");
        assert_eq!(c.len(), 3);
        assert_eq!(
            (c[0].id.as_str(), c[0].heading.as_str(), c[0].text.as_str()),
            ("r1", "Money", "Amounts are int64.")
        );
        assert_eq!(c[2].heading, "Time");
    }

    #[test]
    fn short_notes_stay_whole_and_long_ones_split() {
        let short = Note::parse("---\nid: e1\ntype: error\n---\n## Fix\nDo it.\n");
        assert_eq!(chunks(&short, "x").len(), 1);
        let long = Note::parse(&format!("## A\n{}\n## B\n{}\n", "a".repeat(900), "b".repeat(900)));
        let c = chunks(&long, "n9");
        assert_eq!(c.iter().map(|c| c.heading.as_str()).collect::<Vec<_>>(), vec!["A", "B"]);
        assert_eq!(c[0].id, "n9");
    }
}
