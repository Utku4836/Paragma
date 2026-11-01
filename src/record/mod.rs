//! Knowledge records: rules, decisions and errors (`pg write --rule`,
//! `--decision`, `--error`). Paragma puts each in its place (rules.md,
//! decisions/, errors/) with its fields, and keeps one record per thing:
//! - the same rule, decision or error written again updates the one memory
//!   has (an error counts how often it was seen);
//! - a close error is linked to the one it resembles;
//! - a decision that seems to contradict one on the same subject is pointed
//!   out, with how to mark it as replacing that one;
//! - an error seen often, in many files, is proposed as a rule.

pub mod signature;
pub mod upkeep;

use crate::embed;
use crate::index::{Index, Piece, update_note};
use crate::note::{Kind, Note, Value, rule_line};
use crate::output::{date, now, one_line};
use crate::project::Project;
use anyhow::{Result, bail};

// Similarity is the larger of meaning (cosine) and shared words (Jaccard).
// Measured on pairs: the same rule reworded 0.96, "refund window 30 days"
// and "... 14 days" 0.76, "database is locked (5)" and "sqlite: database
// is locked" 0.86, "checksum mismatch: got <hex>" and "checksum mismatch
// in testdata" 0.66, unrelated pairs below 0.4.

/// A rule this close to an existing one is that rule reworded.
const SAME_RULE: f32 = 0.93;
/// A decision this close is the same decision again.
const SAME_DECISION: f32 = 0.95;
/// Decisions this close are about the same subject: worth checking for a
/// contradiction.
const SAME_SUBJECT: f32 = 0.65;
/// An error this close (or with the same signature) is the same error.
const SAME_ERROR: f32 = 0.92;
/// An error this close is related: linked, not merged.
const RELATED_ERROR: f32 = 0.65;
/// Errors seen this often in all, in at least `PROMOTE_FILES` files, are
/// proposed as a rule.
pub const PROMOTE_SEEN: i64 = 8;
pub const PROMOTE_FILES: usize = 3;

pub const RULES_FILE: &str = "rules.md";
const DEFAULT_GROUP: &str = "General";

/// What a record write did.
pub struct Written {
    pub id: String,
    /// "created", "updated: the same rule as before", ...
    pub what: String,
    /// More worth telling: a possible contradiction, a rule to make.
    pub notes: Vec<String>,
}

pub struct DecisionInput<'a> {
    pub text: &'a str,
    pub title: Option<&'a str>,
    pub why: Option<&'a str>,
    pub alternatives: Option<&'a str>,
    pub supersedes: Option<&'a str>,
}

pub struct ErrorInput<'a> {
    /// The error line as seen.
    pub line: &'a str,
    pub cause: Option<&'a str>,
    pub fix: Option<&'a str>,
    pub files: Vec<String>,
    /// The functions it is about ("Refund", "Cart.Total").
    pub symbols: Vec<String>,
}

pub fn rule(project: &Project, text: &str, group: Option<&str>) -> Result<Written> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" "); // a rule is one line
    let near = nearest(&Index::open(project)?, Kind::Rule, &text)?;
    // never "the same" when one says no where the other does not
    let same = near.as_ref().filter(|p| p.1 >= SAME_RULE && !contradicts(&p.0.text, &text));
    let mut notes = Vec::new();
    if let Some((piece, _)) = near.as_ref().filter(|p| p.1 >= SAME_SUBJECT && contradicts(&p.0.text, &text)) {
        notes.push(format!(
            "may contradict {} ({}); if this one replaces it: pg rm {}",
            piece.id,
            one_line(&piece.text, 70),
            piece.id
        ));
    }
    if let Some((piece, _)) = same {
        update_note(project, &piece.path, |n, _, _| {
            let lines: Vec<String> = n
                .body
                .lines()
                .map(|l| match rule_line(l) {
                    Some((id, _)) if id == piece.id => format!("- {id}: {text}"),
                    _ => l.to_string(),
                })
                .collect();
            n.body = lines.join("\n");
            Ok(())
        })?;
        return Ok(Written { id: piece.id.clone(), what: "updated: the same rule as before".into(), notes });
    }
    let group = group.unwrap_or(DEFAULT_GROUP);
    let (_, id) = update_note(project, RULES_FILE, |n, exists, index| {
        if !exists {
            n.props.set_text("type", Kind::Rule.as_str());
            n.props.set_text("title", "Rules");
        }
        let id = index.next_id(Kind::Rule)?;
        let section = n.section(group).unwrap_or_default();
        let line = format!("- {id}: {text}");
        n.set_section(group, &if section.is_empty() { line } else { format!("{section}\n{line}") });
        Ok(id)
    })?;
    Ok(Written { id, what: "created".into(), notes })
}

pub fn decision(project: &Project, d: &DecisionInput) -> Result<Written> {
    let index = Index::open(project)?;
    let old = match d.supersedes {
        Some(old) => match index.note_by_ref(old)? {
            Some(row) if row.kind == Kind::Decision => Some((old.to_string(), row.path)),
            _ => bail!("no decision {old:?} to supersede (pg find lists decisions)"),
        },
        None => None,
    };
    let title = d.title.map(str::to_string).unwrap_or_else(|| title_of(d.text));
    let near = nearest(&index, Kind::Decision, d.text)?;
    let fill = |n: &mut Note| {
        n.set_section("Decision", d.text);
        if let Some(why) = d.why {
            n.set_section("Why", why);
        }
        if let Some(alt) = d.alternatives {
            n.set_section("Alternatives", alt);
        }
    };
    if old.is_none()
        && let Some((piece, _)) = near.as_ref().filter(|p| p.1 >= SAME_DECISION && !contradicts(&p.0.text, d.text))
    {
        update_note(project, &piece.path, |n, _, _| {
            fill(n);
            Ok(())
        })?;
        return Ok(Written {
            id: piece.id.clone(),
            what: "updated: the same decision as before".into(),
            notes: Vec::new(),
        });
    }
    let mut notes = Vec::new();
    if old.is_none()
        && let Some((piece, _)) = near.as_ref().filter(|p| p.1 >= SAME_SUBJECT && contradicts(&p.0.text, d.text))
    {
        notes.push(format!(
            "may contradict {} ({}); if this one replaces it: pg write --decision \"...\" --supersedes {}",
            piece.id,
            one_line(&piece.text, 70),
            piece.id
        ));
    }
    let rel = free_path(project, "decisions", &title);
    let (id, _) = update_note(project, &rel, |n, _, _| {
        n.props.set_text("type", Kind::Decision.as_str());
        n.props.set_text("title", title.clone());
        n.props.set_text("status", "active");
        n.props.set_text("date", date(now()));
        if let Some((old, _)) = &old {
            n.props.set_text("supersedes", old.clone());
        }
        fill(n);
        Ok(())
    })?;
    if let Some((old, path)) = old {
        update_note(project, &path, |n, _, _| {
            n.props.set_text("status", "superseded");
            n.props.set_text("superseded_by", id.clone());
            Ok(())
        })?;
        notes.push(format!("{old} is now superseded by {id}"));
    }
    Ok(Written { id, what: "created".into(), notes })
}

pub fn error(project: &Project, e: &ErrorInput) -> Result<Written> {
    let signature = signature::normalize(e.line);
    if signature.is_empty() {
        bail!("give the error line: pg write --error \"<the error as printed>\" --cause \"...\" --fix \"...\"");
    }
    let index = Index::open(project)?;
    let near = nearest(&index, Kind::Error, &signature::message(&signature))?;
    let same = match index.error_with_signature(&signature)? {
        Some(row) => Some(row.path),
        None => near.as_ref().filter(|p| p.1 >= SAME_ERROR).map(|p| p.0.path.clone()),
    };
    let today = date(now());
    let merge_files = |n: &mut Note| {
        let mut files = n.props.list("files");
        for f in &e.files {
            if !files.contains(f) {
                files.push(f.clone());
            }
        }
        if !files.is_empty() {
            n.props.set("files", Value::List(files));
        }
        let mut symbols = n.props.list("symbols");
        for s in &e.symbols {
            if !symbols.contains(s) {
                symbols.push(s.clone());
            }
        }
        if !symbols.is_empty() {
            n.props.set("symbols", Value::List(symbols));
        }
    };
    let fill = |n: &mut Note| {
        if let Some(c) = e.cause {
            n.set_section("Cause", c);
        }
        if let Some(f) = e.fix {
            n.set_section("Fix", f);
        }
    };
    if let Some(path) = same {
        let (id, (seen, promote)) = update_note(project, &path, |n, _, index| {
            let seen = n.props.text("seen").and_then(|s| s.parse::<i64>().ok()).unwrap_or(1) + 1;
            n.props.set_text("seen", seen.to_string());
            n.props.set_text("last_seen", today.clone());
            merge_files(n);
            fill(n);
            let promote = promotion(index, n)?;
            if promote.is_some() {
                n.props.set_text("proposed_as_rule", today.clone());
            }
            Ok((seen, promote))
        })?;
        return Ok(Written {
            id,
            what: format!("updated: the same error, seen {seen} times"),
            notes: promote.into_iter().collect(),
        });
    }
    let related = near.filter(|p| p.1 >= RELATED_ERROR).map(|p| p.0.id);
    let title = one_line(&signature, 80);
    let rel = free_path(project, "errors", &title);
    let (id, _) = update_note(project, &rel, |n, _, _| {
        n.props.set_text("type", Kind::Error.as_str());
        n.props.set_text("title", title.clone());
        n.props.set_text("signature", signature.clone());
        merge_files(n);
        n.props.set_text("seen", "1");
        n.props.set_text("last_seen", today.clone());
        n.props.set_text("status", "active");
        if let Some(r) = &related {
            n.props.set("related", Value::List(vec![r.clone()]));
        }
        n.set_section("Symptom", e.line.trim());
        fill(n);
        Ok(())
    })?;
    let mut notes: Vec<String> = related.iter().map(|r| format!("similar to {r}; linked")).collect();
    if related.is_some() {
        // with the errors it is linked to, it may now be seen often enough
        let (_, promote) = update_note(project, &rel, |n, _, index| {
            let promote = promotion(index, n)?;
            if promote.is_some() {
                n.props.set_text("proposed_as_rule", today.clone());
            }
            Ok(promote)
        })?;
        notes.extend(promote);
    }
    Ok(Written { id, what: "created".into(), notes })
}

/// The error note an error line is about: the same signature, else the
/// closest error at least as close as `write --error` links them. Its path.
pub fn known_error(index: &Index, line: &str) -> Result<Option<String>> {
    let signature = signature::normalize(line);
    if let Some(row) = index.error_with_signature(&signature)? {
        return Ok(Some(row.path));
    }
    let near = nearest(index, Kind::Error, &signature::message(&signature))?;
    Ok(near.filter(|p| p.1 >= RELATED_ERROR).map(|p| p.0.path))
}

/// "e45 and similar errors were seen 9 times in 4 files: make it a rule?"
/// once, when an error and the errors linked to it cross the line.
fn promotion(index: &Index, n: &Note) -> Result<Option<String>> {
    let Some(id) = n.id() else { return Ok(None) };
    if n.props.text("proposed_as_rule").is_some() {
        return Ok(None);
    }
    let mine = n.props.list("related");
    let mut seen = n.props.text("seen").and_then(|s| s.parse::<i64>().ok()).unwrap_or(1);
    let mut files = n.props.list("files");
    for other in index.notes_of(Kind::Error)? {
        let (other, props) = (other.id, other.props);
        let linked = props["related"].as_array().is_some_and(|l| l.iter().any(|v| v.as_str() == Some(id)));
        if other == id || !(linked || mine.contains(&other)) {
            continue;
        }
        seen += props["seen"].as_str().and_then(|s| s.parse::<i64>().ok()).unwrap_or(1);
        for f in props["files"].as_array().into_iter().flatten().filter_map(|v| v.as_str()) {
            if !files.iter().any(|x| x == f) {
                files.push(f.to_string());
            }
        }
    }
    if seen < PROMOTE_SEEN || files.len() < PROMOTE_FILES {
        return Ok(None);
    }
    let fix = n.section("Fix").map(|f| one_line(&f, 80)).unwrap_or_default();
    Ok(Some(format!(
        "{id} and the errors linked to it were seen {seen} times in {} files: make it a rule? pg write --rule \"{}\"",
        files.len(),
        if fix.is_empty() { "..." } else { &fix }
    )))
}

/// The piece of a kind closest in meaning to `text`, with its similarity.
fn nearest(index: &Index, kind: Kind, text: &str) -> Result<Option<(Piece, f32)>> {
    // the index's vectors also carry the note's title and section; compare
    // plain text with plain text, by meaning and by the words they share
    let enc = embed::encoder();
    let v = enc.encode(text);
    let mine = words(text);
    let best = index
        .pieces_of(kind)?
        .into_iter()
        .map(|p| {
            let plain = what_it_says(kind, &p.text);
            let s = embed::similarity(&v, &enc.encode(&plain)).max(overlap(&mine, &words(&plain)));
            (p, s)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1));
    if std::env::var_os("PG_EXPLAIN").is_some()
        && let Some((p, s)) = &best
    {
        eprintln!("explain: nearest {} {} at {s:.3}", kind.as_str(), p.id);
    }
    Ok(best)
}

/// The part of a record that says what it is: a decision's decision, an
/// error's signature, a rule's text.
fn what_it_says(kind: Kind, text: &str) -> String {
    let note = Note::parse(text);
    let section = match kind {
        Kind::Decision => note.section("Decision"),
        Kind::Error => note.section("Symptom").map(|s| signature::message(&signature::normalize(&s))),
        _ => None,
    };
    section.unwrap_or_else(|| text.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join(" "))
}

/// A text's words: lower case, accents off (kuruş = kurus).
fn words(text: &str) -> Vec<String> {
    use unicode_normalization::UnicodeNormalization;
    let folded: String = text
        .to_lowercase()
        .replace('ı', "i")
        .nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect();
    let mut w: Vec<String> =
        folded.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect();
    w.sort();
    w.dedup();
    w
}

/// Shared words over all words of the two (Jaccard).
fn overlap(a: &[String], b: &[String]) -> f32 {
    let shared = a.iter().filter(|w| b.contains(w)).count();
    let all = a.len() + b.len() - shared;
    if all == 0 { 0.0 } else { shared as f32 / all as f32 }
}

/// Two texts on the same subject that seem to say different things: other
/// numbers, or one says no where the other does not.
fn contradicts(a: &str, b: &str) -> bool {
    let numbers = |s: &str| -> Vec<String> {
        let mut n: Vec<String> =
            s.split(|c: char| !c.is_ascii_digit()).filter(|w| !w.is_empty()).map(str::to_string).collect();
        n.sort();
        n.dedup();
        n
    };
    let negative = |s: &str| {
        const NO: &[&str] =
            &["not", "never", "no", "don't", "dont", "without", "değil", "asla", "yok", "hiçbir", "olmadan"];
        s.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '\'').any(|w| NO.contains(&w))
    };
    let (na, nb) = (numbers(a), numbers(b));
    (!na.is_empty() && !nb.is_empty() && na != nb) || negative(a) != negative(b)
}

/// A short title: the first sentence, cut at a word near 60 characters.
fn title_of(text: &str) -> String {
    let first = text.split(['.', ';', '\n']).map(str::trim).find(|s| !s.is_empty()).unwrap_or("");
    if first.chars().count() <= 60 {
        return first.to_string();
    }
    let mut out = String::new();
    for w in first.split_whitespace() {
        if out.chars().count() + w.chars().count() > 60 {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

/// A file for a new record in `folder`, named after its title and not
/// taken yet.
fn free_path(project: &Project, folder: &str, title: &str) -> String {
    let slug = crate::project::names::slug(title);
    // without the signature's marks (<n>, <hex>, <id>)
    let name: Vec<&str> = slug.split('-').filter(|w| !matches!(*w, "n" | "hex" | "id")).take(6).collect();
    let base = if name.is_empty() { "record".to_string() } else { name.join("-") };
    let mut rel = format!("{folder}/{base}.md");
    let mut n = 2;
    while project.dir.join(&rel).exists() {
        rel = format!("{folder}/{base}-{n}.md");
        n += 1;
    }
    rel
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contradictions() {
        assert!(contradicts("Refund window is 30 days.", "Refund window is 14 days."));
        assert!(contradicts("Discounts apply before VAT.", "Discounts do not apply before VAT."));
        assert!(!contradicts("Use ULIDs for public ids.", "Public ids are ULIDs."));
    }

    #[test]
    fn titles() {
        assert_eq!(title_of("Use ULIDs for public IDs. They sort by time."), "Use ULIDs for public IDs");
        let long = "Discount codes are validated on the server only and the client never computes any price at all";
        assert!(title_of(long).chars().count() <= 60);
    }
}
