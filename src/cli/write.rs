//! pg write: create or update a note, a folder or a project.
//!
//! ```text
//! pg write physics/collisions "Use swept AABB."   note in the current project
//! pg write physics/                               folder
//! pg write game-engine/                           project (when none is current)
//! pg write shop/payments/refunds "..."            note in another project
//!
//! pg write --rule "Amounts are int64 kuruş, never float" --group Money
//! pg write --decision "Discounts apply before VAT" --why "finance asked"
//! pg write --error "catalog checksum mismatch" --cause "CRLF on Windows" --fix "normalise line endings"
//! ```
//!
//! Rules, decisions and errors go to their place in the project with their
//! fields, and are never written twice (see record/).
//!
//! pg write --secret "Npm Api" <value>   a secret, by its title, to the
//! project's env file: never shown again, used by pg run as $NPM_API.
//! Secrets in any other text are masked and not kept: they need a title.

use super::{Context, NOT_FOUND, USAGE, fail};
use crate::index::update_note;
use crate::note::Kind;
use crate::project::{Project, check_name};
use crate::record::{self, DecisionInput, ErrorInput, Written};
use crate::state::State;
use anyhow::Result;
use std::io::Read;

#[derive(clap::Args)]
#[command(after_help = "Examples:
  pg write setup/dev-server \"How the dev server starts: ...\"   a note (made or replaced)
  pg write setup/dev-server --append \"One more thing\"
  pg write setup/dev-server --file notes.md
  pg write --rule \"Tests run with --release\" --group Tests
  pg write --decision \"Postgres, not SQLite\" --why \"several writers\"
  pg write --error \"<the error line>\" --cause \"...\" --fix \"...\"
  pg write --secret \"Stripe key\" sk_test_...   then $STRIPE_KEY in pg run")]
pub struct Args {
    /// Where: note, folder/ or project/ (a name ending in '/' is a folder)
    path: Option<String>,

    /// The note's text ("-" reads it from stdin)
    text: Vec<String>,

    /// Add the text at the end instead of replacing the body
    #[arg(long)]
    append: bool,

    /// A file: a text file's text is the note's; any other file (a workbook, a PDF, an image) is kept with the note
    #[arg(long, value_name = "FILE")]
    file: Option<std::path::PathBuf>,

    /// With --file: keep the file itself with the note, also a text file
    #[arg(long, requires = "file")]
    attach: bool,

    /// Keep a secret (API key, token, password) under this title in the project's env file;
    /// the value follows, or "-" reads it from stdin
    #[arg(long, value_name = "TITLE", conflicts_with_all = ["rule", "decision", "error"])]
    secret: Option<String>,

    /// Record a rule (to rules.md)
    #[arg(long, value_name = "TEXT", conflicts_with_all = ["decision", "error"], allow_hyphen_values = true)]
    rule: Option<String>,

    /// The rule's group in rules.md (default: General)
    #[arg(long, requires = "rule")]
    group: Option<String>,

    /// Record a decision (to decisions/)
    #[arg(long, value_name = "TEXT", conflicts_with = "error", allow_hyphen_values = true)]
    decision: Option<String>,

    /// Why it was decided
    #[arg(long, requires = "decision", allow_hyphen_values = true)]
    why: Option<String>,

    /// What else was considered
    #[arg(long, requires = "decision", allow_hyphen_values = true)]
    alternatives: Option<String>,

    /// The decision this one replaces (its id)
    #[arg(long, value_name = "ID", requires = "decision")]
    supersedes: Option<String>,

    /// A short title (default: the first sentence)
    #[arg(long, allow_hyphen_values = true)]
    title: Option<String>,

    /// Record an error: the error line as printed (to errors/)
    #[arg(long, value_name = "LINE", allow_hyphen_values = true)]
    error: Option<String>,

    /// What caused the error
    #[arg(long, requires = "error", allow_hyphen_values = true)]
    cause: Option<String>,

    /// How it was fixed
    #[arg(long, requires = "error", allow_hyphen_values = true)]
    fix: Option<String>,

    /// Files the error is about, comma separated
    #[arg(long, value_delimiter = ',', requires = "error")]
    files: Vec<String>,

    /// Say a record marked "may be outdated" is still right: it keeps the
    /// code as it is now
    #[arg(long, value_name = "ID", conflicts_with_all = ["rule", "decision", "error", "secret"])]
    confirm: Option<String>,

    /// A due date for the note: 2026-12-01, +7d, +2w; none takes it off (pg ls --due lists them)
    #[arg(long, value_name = "DATE")]
    due: Option<String>,

    /// The note's table: add "a | b | c" (a row), sort <column> (desc: -<column>), sum <column> (a total row)
    #[arg(long, num_args = 1.., value_name = "ACTION", allow_hyphen_values = true)]
    table: Vec<String>,

    /// With --table: the table under this heading (default: the note's first)
    #[arg(long, value_name = "HEADING", requires = "table")]
    under: Option<String>,

    /// Tick off a to-do: <note id>.<number> as pg ls --todo gives it (n4.2); several with commas
    #[arg(long, value_name = "TODO", value_delimiter = ',', conflicts_with_all = ["rule", "decision", "error", "secret"])]
    check: Vec<String>,

    /// Open a ticked to-do again: <note id>.<number>
    #[arg(long, value_name = "TODO", value_delimiter = ',')]
    uncheck: Vec<String>,
}

pub fn run(cx: &Context, mut a: Args) -> Result<()> {
    if let Some(id) = a.confirm.clone() {
        return confirm(cx, &id);
    }
    if !a.check.is_empty() || !a.uncheck.is_empty() {
        return check(cx, &a.check, &a.uncheck);
    }
    if a.due.is_some() || !a.table.is_empty() {
        return note_items(cx, &a);
    }
    if let Some(f) = &a.file
        && (a.attach || !is_text(f))
    {
        return attach(cx, &a, f);
    }
    if let Some(title) = a.secret.clone() {
        return write_secret(cx, &title, &a);
    }
    if a.rule.is_some() || a.decision.is_some() || a.error.is_some() {
        let fields = [
            &mut a.rule,
            &mut a.decision,
            &mut a.why,
            &mut a.alternatives,
            &mut a.title,
            &mut a.error,
            &mut a.cause,
            &mut a.fix,
        ];
        let warning = super::mask_all(fields.into_iter().flatten());
        write_record(cx, &a)?;
        if let Some(w) = warning {
            println!("{w}");
        }
        return Ok(());
    }
    let Some(path) = a.path.as_deref() else {
        return Err(fail(USAGE, "say where: pg write <note> \"text\" (or --rule, --decision, --error)"));
    };
    let mut text = text_of(&a)?;
    let warning = super::mask_all([&mut text]);
    let folder_only = path.ends_with('/');
    let (project, rel) = target(cx, path.trim_end_matches('/'))?;
    for part in rel.split('/').filter(|s| !s.is_empty()) {
        check_name(part.trim_end_matches(".md")).map_err(|e| fail(USAGE, e.to_string()))?;
    }
    let project = match cx.home.project(&project) {
        Some(p) => p,
        None => cx.home.create_project(&project).map_err(|e| fail(USAGE, e.to_string()))?,
    };
    if rel.is_empty() {
        println!("ok {} (project)", project.name);
        return Ok(());
    }
    if folder_only {
        std::fs::create_dir_all(project.dir.join(&rel))?;
        println!("ok {}/{rel}/ (folder)", project.name);
        return Ok(());
    }
    let (id, what) = write_note(&project, &rel, &text, a.append)?;
    State::open(&project)?.touch_session(&cx.cwd.to_string_lossy())?;
    println!("ok {}/{} ({id}, {what})", project.name, rel.trim_end_matches(".md"));
    // "notes/x" in shop, when a project is called notes: said aloud, so a
    // note meant for this project is not lost in the other one unseen
    if let Some(here) = &cx.project
        && here.name != project.name
    {
        println!(
            "written in project {}, not {}: {} has a project of that name; this project's own folder: pg write {}/{path} ...",
            project.name,
            here.name,
            path.split('/').next().unwrap_or(""),
            here.name
        );
    }
    if let Some(w) = warning {
        println!("{w}");
    }
    if let Some(h) = topic_hint(&project, &rel) {
        println!("{h}");
    }
    // a note that has grown to hold unrelated topics: one note each
    if let Some(t) = super::sleep::topics_of(&project, &rel) {
        println!("{t}");
    }
    Ok(())
}

/// Whether a file is text to put in a note (UTF-8, no NUL, not a document).
fn is_text(path: &std::path::Path) -> bool {
    if crate::docs::Kind::of(path).is_some() {
        return false;
    }
    let Ok(bytes) = std::fs::read(path) else { return true }; // its error comes when it is read
    // text: UTF-8 with no NUL, and hardly any control character but tabs and line ends
    std::str::from_utf8(&bytes).is_ok_and(|t| {
        let control = t.chars().filter(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r')).count();
        !t.contains('\0') && control * 100 <= t.chars().count().max(1)
    })
}

/// A file kept with a note; said what was done and whether it goes with
/// the memory to other computers.
fn attach(cx: &Context, a: &Args, file: &std::path::Path) -> Result<()> {
    use crate::project::attach::{Kept, keep, size};
    let Some(path) = a.path.as_deref() else {
        return Err(fail(USAGE, "say which note it goes with: pg write <note> --file <path>"));
    };
    if !file.is_file() {
        return Err(fail(NOT_FOUND, format!("no file {}", file.display())));
    }
    let (project, rel) = target(cx, path.trim_end_matches('/'))?;
    let project = match cx.home.project(&project) {
        Some(p) => p,
        None => cx.home.create_project(&project).map_err(|e| fail(USAGE, e.to_string()))?,
    };
    let kept = keep(&project, &rel, file).map_err(|e| fail(USAGE, e.to_string()))?;
    let note = rel.trim_end_matches(".md");
    let bytes = match &kept {
        Kept::New { name, size: s } => {
            println!(
                "ok {}/{note}: {name} kept with it ({}); the note lists it under Attachments",
                project.name,
                size(*s)
            );
            *s
        }
        Kept::Version { name, size: s, old } => {
            println!(
                "ok {}/{note}: {name} changed, kept as the new version ({}); the old one is {old}",
                project.name,
                size(*s)
            );
            *s
        }
        Kept::Same { name } => {
            println!("ok {}/{note}: {name} is kept already, the same (nothing copied)", project.name);
            return Ok(());
        }
    };
    State::open(&project)?.touch_session(&cx.cwd.to_string_lossy())?;
    State::open(&project)?.record(crate::state::journal::Did::Write {
        kind: "attach".into(),
        id: String::new(),
        what: format!("{note} {}", file.display()),
    })?;
    // whether it goes with the memory to the other computers
    match crate::sync::Config::load(&cx.home) {
        None => {}
        Some(c) if !c.attachments => {
            println!("attachments are not synced: it stays on this computer (pg git turns them on)")
        }
        Some(c) if bytes > c.attach_mb * 1024 * 1024 => println!(
            "it stays on this computer: {} is over the sync limit of {} MB (pg git changes the limit)",
            size(bytes),
            c.attach_mb
        ),
        Some(_) => println!("it goes with the memory to your other computers at the next sync"),
    }
    Ok(())
}

/// A to-do ticked off or opened again, by "<note id>.<number>".
fn check(cx: &Context, done: &[String], open: &[String]) -> Result<()> {
    for (which, to) in done.iter().map(|w| (w, true)).chain(open.iter().map(|w| (w, false))) {
        let (id, n) =
            which.trim().rsplit_once('.').and_then(|(id, n)| Some((id, n.parse::<usize>().ok()?))).ok_or_else(
                || {
                    fail(
                        USAGE,
                        format!("give the to-do as pg ls --todo shows it: <note id>.<number> (n4.2), not {which:?}"),
                    )
                },
            )?;
        let (project, row) = super::read::locate(cx, id)?;
        let (_, text) =
            update_note(&project, &row.path, |note, _, _| Ok(crate::project::items::set_todo(&mut note.body, n, to)))?;
        match text {
            Some(t) => println!("ok {id}.{n} {}: {t}", if to { "done" } else { "open again" }),
            None => {
                return Err(fail(
                    NOT_FOUND,
                    format!("{id} has no to-do {n} (pg ls --todo lists them with their numbers)"),
                ));
            }
        }
    }
    Ok(())
}

/// A note's due date or its table changed: the note must be there.
fn note_items(cx: &Context, a: &Args) -> Result<()> {
    use crate::project::items;
    let Some(path) = a.path.as_deref() else {
        return Err(fail(USAGE, "say which note: pg write <note> --due 2026-12-01 (or --table ...)"));
    };
    let (project, row) = super::read::locate(cx, path)?;
    let shown = row.path.trim_end_matches(".md").to_string();
    if let Some(given) = &a.due {
        let due = items::due_of(given).ok_or_else(|| {
            fail(USAGE, format!("give the date as 2026-12-01, +7d or +2w (none takes it off), not {given:?}"))
        })?;
        update_note(&project, &row.path, |n, _, _| {
            if due.is_empty() {
                n.props.remove("due");
            } else {
                n.props.set_text("due", due.clone());
            }
            Ok(())
        })?;
        match items::days_left(&due) {
            Some(d) => println!("ok {shown} is due {due} ({})", items::when(d)),
            None => println!("ok {shown} has no due date now"),
        }
    }
    if a.table.is_empty() {
        return Ok(());
    }
    let action = a.table[0].to_lowercase();
    let value = a.table[1..].join(" ");
    let under = a.under.as_deref();
    let (_, said) = update_note(&project, &row.path, |n, _, _| {
        let Some(t) = items::table(&n.body, under) else {
            return Ok(Err(match under {
                Some(h) => format!("{shown} has no table under {h:?}"),
                None => format!("{shown} has no table: write one in it first (| a | b |, then |---|---|)"),
            }));
        };
        let mut new = t.clone();
        let msg = match action.as_str() {
            "add" => {
                let row: Vec<String> =
                    value.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect();
                if row.iter().all(|c| c.is_empty()) {
                    return Ok(Err("give the row: --table add \"a | b | c\"".to_string()));
                }
                // before pg's total row, if there is one
                let at = new.rows.iter().position(|r| items::is_total(r)).unwrap_or(new.rows.len());
                new.rows.insert(at, row);
                format!("a row added ({} now)", new.rows.len())
            }
            "sort" => {
                let (desc, name) = match value.trim().strip_prefix('-') {
                    Some(n) => (true, n),
                    None => (false, value.trim()),
                };
                let Some(k) = items::column(&t, name) else {
                    return Ok(Err(format!("no column {name:?}: {}", t.header.join(", "))));
                };
                let total: Vec<Vec<String>> = new.rows.iter().filter(|r| items::is_total(r)).cloned().collect();
                new.rows.retain(|r| !items::is_total(r));
                new.rows.sort_by(|x, y| {
                    let (a, b) =
                        (x.get(k).map(String::as_str).unwrap_or(""), y.get(k).map(String::as_str).unwrap_or(""));
                    let o = match (items::number(a), items::number(b)) {
                        (Some(p), Some(q)) => p.partial_cmp(&q).unwrap_or(std::cmp::Ordering::Equal),
                        _ => a.to_lowercase().cmp(&b.to_lowercase()),
                    };
                    if desc { o.reverse() } else { o }
                });
                new.rows.extend(total);
                format!("sorted by {}{}", t.header[k], if desc { ", highest first" } else { "" })
            }
            "sum" => {
                let Some(k) = items::column(&t, value.trim()) else {
                    return Ok(Err(format!("no column {:?}: {}", value.trim(), t.header.join(", "))));
                };
                new.rows.retain(|r| !items::is_total(r));
                let sum: f64 = new.rows.iter().filter_map(|r| r.get(k).and_then(|c| items::number(c))).sum();
                // the decimal mark the column uses
                let comma = new.rows.iter().filter_map(|r| r.get(k)).any(|c| items::decimal_comma(c));
                let mut total = vec![String::new(); t.header.len()];
                total[0] = "**Total**".into();
                let shown = if comma { items::cell_of(sum).replace('.', ",") } else { items::cell_of(sum) };
                if k > 0 {
                    total[k] = shown.clone();
                } else {
                    total[0] = format!("**Total** {shown}");
                }
                new.rows.push(total);
                format!("{} total: {shown}", t.header[k])
            }
            other => return Ok(Err(format!("--table takes add, sort or sum, not {other:?}"))),
        };
        n.body = items::replace(&n.body, &t, &new);
        Ok(Ok(msg))
    })?;
    match said {
        Ok(m) => println!("ok {shown}: {m}"),
        Err(e) => return Err(fail(USAGE, e)),
    }
    Ok(())
}

/// A folder is a category of notes: its first note says so, so that the
/// next topic of it goes in a note of its own.
pub fn topic_hint(project: &crate::project::Project, rel: &str) -> Option<String> {
    let rel = if rel.ends_with(".md") { rel.to_string() } else { format!("{rel}.md") };
    let text = std::fs::read_to_string(project.dir.join(&rel)).ok()?;
    if !plain_note(&text) {
        return None;
    }
    let (folder, _) = rel.rsplit_once('/')?;
    let notes = std::fs::read_dir(project.dir.join(folder))
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".md"))
        .count();
    (notes == 1).then(|| {
        format!("{folder}/ is a category: each other topic of it goes in its own note there (pg write {folder}/<topic> \"...\")")
    })
}

/// A note people write freely: not the project note, rules, a decision or
/// an error (those have their own sections).
pub fn plain_note(text: &str) -> bool {
    let note = crate::note::Note::parse(text);
    !matches!(note.props.text("type"), Some("project" | "rule" | "rules" | "decision" | "error"))
}

/// A secret to the project's env file, by its title.
fn write_secret(cx: &Context, title: &str, a: &Args) -> Result<()> {
    let project = cx.need_project()?;
    let given: Vec<String> = a.path.iter().chain(a.text.iter()).cloned().collect();
    let value = if given.len() == 1 && given[0] == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        s.trim_end_matches(['\r', '\n']).to_string()
    } else {
        given.join(" ")
    };
    if title.trim().is_empty() || value.trim().is_empty() {
        return Err(fail(USAGE, "a secret needs a title and a value: pg write --secret \"Npm Api\" <value>"));
    }
    let (s, new) = crate::project::env::set(project, title, value.trim())?;
    State::open(project)?.record(crate::state::journal::Did::Secret { title: s.title.clone(), new })?;
    println!(
        "ok {} {}: use ${} in pg run (pg run -- <command with ${}>)",
        s.show(),
        if new { "kept" } else { "replaced" },
        s.name,
        s.name
    );
    Ok(())
}

/// A rule, a decision or an error, put in its place by record/.
fn write_record(cx: &Context, a: &Args) -> Result<()> {
    if a.path.is_some() || !a.text.is_empty() {
        return Err(fail(
            USAGE,
            "a rule, decision or error goes to its own place: give its text to --rule, --decision or --error",
        ));
    }
    let project = cx.need_project()?;
    let usage = |e: anyhow::Error| fail(USAGE, e.to_string());
    let (kind, w): (&str, Written) = if let Some(text) = &a.rule {
        ("rule", record::rule(project, text, a.group.as_deref())?)
    } else if let Some(text) = &a.decision {
        let d = DecisionInput {
            text,
            title: a.title.as_deref(),
            why: a.why.as_deref(),
            alternatives: a.alternatives.as_deref(),
            supersedes: a.supersedes.as_deref(),
            commit: crate::code::head_commit(&cx.home, project),
        };
        ("decision", record::decision(project, &d).map_err(usage)?)
    } else {
        let line = a.error.as_deref().unwrap_or_default();
        let (files, symbols) =
            if a.files.is_empty() { code_of(cx, project, line)? } else { (a.files.clone(), Vec::new()) };
        let commit = crate::code::head_commit(&cx.home, project);
        let code_state = match crate::code::notes::open(&cx.home, project)? {
            Some(code) => crate::code::stale::state_of(&code, &files, &symbols)?,
            None => None,
        };
        let e = ErrorInput {
            line,
            cause: a.cause.as_deref(),
            fix: a.fix.as_deref(),
            files,
            symbols,
            commit,
            code_state,
            draft: false,
        };
        ("error", record::error(project, &e).map_err(usage)?)
    };
    let state = State::open(project)?;
    state.record(crate::state::journal::Did::Write {
        kind: kind.to_string(),
        id: w.id.clone(),
        what: w.what.clone(),
    })?;
    state.touch_session(&cx.cwd.to_string_lossy())?;
    println!("ok {} ({kind} {})", w.id, w.what);
    for n in &w.notes {
        println!("{n}");
    }
    Ok(())
}

/// The files and functions of the project's code an error line points at
/// (file:line), for an error written without --files.
fn code_of(cx: &Context, project: &Project, line: &str) -> Result<(Vec<String>, Vec<String>)> {
    let (mut files, mut symbols) = (Vec::new(), Vec::new());
    let places = crate::testrun::parse::locations(line);
    if places.is_empty() {
        return Ok((files, symbols));
    }
    let Some(code) = crate::code::notes::open(&cx.home, project)? else { return Ok((files, symbols)) };
    for (file, n) in places {
        if let Some((folder, path)) = code.locate(&file)? {
            if let Some(s) = code.at(&folder, &path, n)? {
                symbols.push(s.qualified);
            }
            files.push(code.display(&folder, &path));
        }
    }
    Ok((files, symbols))
}

/// Writes a plain note (replacing its body, or appending to it).
pub fn write_note(project: &Project, rel: &str, text: &str, append: bool) -> Result<(String, &'static str)> {
    let (id, what) = update_note(project, rel, |n, exists, _| {
        if !exists {
            let title = rel.trim_end_matches(".md").rsplit('/').next().unwrap_or(rel).to_string();
            n.props.set_text("type", Kind::Note.as_str());
            n.props.set_text("title", title);
            n.body = text.to_string();
            return Ok("created");
        }
        if append {
            let body = n.body.trim_end().to_string();
            n.body = if body.is_empty() { text.to_string() } else { format!("{body}\n\n{text}") };
            Ok("appended")
        } else {
            n.body = text.to_string();
            Ok("updated")
        }
    })?;
    State::open(project)?.record(crate::state::journal::Did::Write {
        kind: project.rel(&project.note_file(rel)),
        id: id.clone(),
        what: what.to_string(),
    })?;
    Ok((id, what))
}

/// A record said to be still right after its code changed: it keeps the
/// code as it is now (the hashes and the commit), so it is not marked again
/// until that code changes again.
fn confirm(cx: &Context, id: &str) -> Result<()> {
    let (project, row) = super::read::locate(cx, id)?;
    // a draft rule pg wrote: confirmed, it is a rule (out of the drafts)
    if row.path == crate::record::RULES_FILE {
        let (_, moved) = update_note(&project, &row.path, |n, _, _| {
            let mut text = None;
            let lines: Vec<String> = n
                .body
                .lines()
                .filter(|l| match crate::note::rule_line(l) {
                    Some((rid, t)) if rid == id && t.starts_with("(draft)") => {
                        text = Some(t.trim_start_matches("(draft)").trim().to_string());
                        false
                    }
                    _ => true,
                })
                .map(str::to_string)
                .collect();
            let Some(text) = text else { return Ok(false) };
            n.body = lines.join("\n");
            if n.section(super::act::DRAFT_RULES).is_some_and(|s| s.trim().is_empty()) {
                n.remove_section(super::act::DRAFT_RULES);
            }
            let section = n.section("General").unwrap_or_default();
            let line = format!("- {id}: {text}");
            n.set_section("General", &if section.trim().is_empty() { line } else { format!("{section}\n{line}") });
            Ok(true)
        })?;
        if moved {
            State::open(&project)?.record(crate::state::journal::Did::Write {
                kind: "confirm".into(),
                id: id.to_string(),
                what: "confirmed the draft rule".into(),
            })?;
            println!("ok {id} (a rule now; reword it: pg write --rule \"...\" and pg rm {id})");
            return Ok(());
        }
    }
    // a draft pg wrote: confirming keeps it as a record
    let path = project.dir.join(&row.path);
    let status =
        crate::note::Note::parse(&crate::project::files::read_text(&path)?).props.text("status").map(str::to_string);
    // archived for being unused: in use again
    if status.as_deref() == Some("archived") {
        update_note(&project, &row.path, |n, _, _| {
            n.props.set_text("status", "active");
            n.props.set_text("last_seen", crate::output::date(crate::output::now()));
            Ok(())
        })?;
        State::open(&project)?.record(crate::state::journal::Did::Write {
            kind: "confirm".into(),
            id: id.to_string(),
            what: "back in use".into(),
        })?;
        println!("ok {id} (back in use: no longer archived)");
        return Ok(());
    }
    if status.as_deref() == Some("draft") {
        update_note(&project, &row.path, |n, _, _| {
            n.props.set_text("status", "active");
            Ok(())
        })?;
        State::open(&project)?.record(crate::state::journal::Did::Write {
            kind: "confirm".into(),
            id: id.to_string(),
            what: "confirmed the draft".into(),
        })?;
        println!("ok {id} (the draft is a record now; its cause: pg write --error \"<its line>\" --cause \"...\")");
        return Ok(());
    }
    let Some(code) = crate::code::notes::open(&cx.home, &project)? else {
        return Err(fail(USAGE, "no code folder is linked to this project: nothing to compare the record with"));
    };
    let note = crate::note::Note::parse(&crate::project::files::read_text(&project.dir.join(&row.path))?);
    let props = crate::index::Index::open(&project)?.props_at(&row.path);
    let drift = crate::code::stale::Staleness::new(&cx.home, &project).drift(&props)?.unwrap_or_default();
    if !drift.gone.is_empty() {
        return Err(fail(
            USAGE,
            format!(
                "{} {} gone: change the record (pg write) or take it out (pg rm {id}); confirming keeps nothing",
                drift.gone.join(", "),
                if drift.gone.len() == 1 { "is" } else { "are" }
            ),
        ));
    }
    // the places it names: its code_state's, else its files and symbols
    let mut files = note.props.list("files");
    let mut symbols = note.props.list("symbols");
    if let Some(state) = note.props.text("code_state") {
        for e in state.split(',') {
            let place = e.trim().rsplit_once('=').map(|(p, _)| p).unwrap_or("");
            match place.split_once('#') {
                Some((f, s)) => {
                    files.push(f.to_string());
                    symbols.push(s.to_string());
                }
                None if !place.is_empty() => files.push(place.to_string()),
                None => {}
            }
        }
    }
    let state = crate::code::stale::state_of(&code, &files, &symbols)?;
    let commit = crate::code::head_commit(&cx.home, &project);
    update_note(&project, &row.path, |n, _, _| {
        if let Some(s) = &state {
            n.props.set_text("code_state", s.clone());
        }
        if let Some(c) = &commit {
            n.props.set_text("commit", c.clone());
        }
        Ok(())
    })?;
    State::open(&project)?.record(crate::state::journal::Did::Write {
        kind: "confirm".into(),
        id: id.to_string(),
        what: "confirmed".into(),
    })?;
    println!("ok {id} (confirmed: it keeps the code as it is now)");
    Ok(())
}

/// The project and the path in it a write is aimed at.
fn target(cx: &Context, path: &str) -> Result<(String, String)> {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return Err(fail(USAGE, "say where: pg write <note> \"text\" (or folder/, or project/)"));
    }
    let (first, rest) = path.split_once('/').unwrap_or((path, ""));
    match &cx.project {
        // inside the current project, unless the path names another project
        // and this one has no folder of that name
        Some(p)
            if first != p.name
                && !rest.is_empty()
                && cx.home.project(first).is_some()
                && !p.dir.join(first).is_dir() =>
        {
            Ok((first.to_string(), rest.to_string()))
        }
        Some(p) if first == p.name && !rest.is_empty() => Ok((p.name.clone(), rest.to_string())),
        Some(p) => Ok((p.name.clone(), path.to_string())),
        // no current project: the first part is the project
        None => {
            check_name(first).map_err(|e| fail(USAGE, e.to_string()))?;
            Ok((first.to_string(), rest.to_string()))
        }
    }
}

fn text_of(a: &Args) -> Result<String> {
    if let Some(f) = &a.file {
        return Ok(std::fs::read_to_string(f)?);
    }
    if a.text.len() == 1 && a.text[0] == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        return Ok(s);
    }
    Ok(a.text.join(" "))
}
