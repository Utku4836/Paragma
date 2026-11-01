//! pg read: print a note, or a piece of the project's code.
//!
//! ```text
//! pg read e45                       a note by id (r3: one rule)
//! pg read physics/collisions        a note by path
//! pg read payments/refund.go#Refund one function, with the notes about it
//! pg read payments/refund.go:40-60  lines of a file
//! ```

use super::{Context, NOT_FOUND, fail};
use crate::code::lang::Lang;
use crate::code::notes::CodeNotes;
use crate::code::{self, CodeIndex};
use crate::index::{Index, NoteRow};
use crate::note::{Note, Value, chunks};
use crate::output::Budget;
use crate::project::{Project, files};
use crate::state::State;
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// A note id (e45), a rule id (r3), a note path, or code: file#Function, file:10-20, file
    target: String,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let looks_like_code = a.target.contains('#') || lines_of(&a.target).is_some() || Lang::of(&a.target).is_some();
    if looks_like_code && let Some(done) = read_code(cx, &a.target)? {
        print!("{done}");
        return Ok(());
    }
    let (project, row) = match locate(cx, &a.target) {
        Ok(found) => found,
        Err(e) => match read_code(cx, &a.target)? {
            Some(done) => {
                print!("{done}");
                return Ok(());
            }
            None => return Err(e),
        },
    };
    let note = Note::parse(&files::read_text(&project.dir.join(&row.path))?);
    let mut b = Budget::new(cx.budget);

    // a rule id: just that rule
    let id = a.target.rsplit(':').next().unwrap_or(&a.target);
    if row.id != id
        && let Some(c) = chunks(&note, &row.path).into_iter().find(|c| c.id == id)
    {
        b.add(&format!("[{id} rule] {}#{}\n{}", row.path.trim_end_matches(".md"), c.heading, c.text));
        print!("{}", b.out);
        let _ = State::open(&project).and_then(|s| s.shown(&[id.to_string()], true));
        return Ok(());
    }
    b.add(&format!(
        "[{} {}] {}",
        if row.id.is_empty() { "-" } else { &row.id },
        row.kind.as_str(),
        row.path.trim_end_matches(".md")
    ));
    let props = Index::open(&project)?.props_at(&row.path);
    if let Some(w) = crate::code::stale::Staleness::new(&cx.home, &project).check(&props)? {
        b.add(&format!("({w})"));
    }
    for (k, v) in note.props.iter() {
        if matches!(k.as_str(), "id" | "type" | "title") {
            continue;
        }
        let v = match v {
            Value::Text(s) => s.clone(),
            Value::List(l) => l.join(", "),
        };
        if !v.is_empty() {
            b.add(&format!("{k}: {v}"));
        }
    }
    if !b.add(note.body.trim()) {
        // too long for the budget: as much as fits, then where the rest is
        let mut lines = note.body.lines();
        let mut n = 0;
        for l in lines.by_ref() {
            if !b.add(l) {
                break;
            }
            n += 1;
        }
        b.out.push_str(&format!("... more below line {n}: pg read {} --budget {}\n", a.target, cx.budget * 2));
    }
    print!("{}", b.out);
    if !row.id.is_empty() {
        // later searches in this conversation show it as one line
        let _ = State::open(&project).and_then(|s| s.shown(std::slice::from_ref(&row.id), true));
    }
    Ok(())
}

/// The note a target names: "e45", "shop:e45", "physics/collisions" or
/// "shop/physics/collisions".
pub fn locate(cx: &Context, target: &str) -> Result<(Project, NoteRow)> {
    let (proj_name, rest) = match target.split_once(':') {
        Some((p, r)) if cx.home.project(p).is_some() => (Some(p.to_string()), r.to_string()),
        _ => (None, target.to_string()),
    };
    let mut candidates: Vec<(Project, String)> = Vec::new();
    match (&proj_name, &cx.project) {
        (Some(p), _) => candidates.push((cx.home.project(p).expect("checked above"), rest.clone())),
        (None, _) => {
            // "shop/physics/collisions": the first part may be a project
            if let Some((first, inner)) = rest.split_once('/')
                && let Some(p) = cx.home.project(first)
            {
                candidates.push((p, inner.to_string()));
            }
            if let Some(p) = &cx.project {
                candidates.push((p.clone(), rest.clone()));
            }
            if cx.project.is_none() {
                for p in cx.home.projects() {
                    candidates.push((p, rest.clone()));
                }
            }
        }
    }
    for (p, r) in candidates {
        let index = Index::open(&p)?;
        let path = if r.ends_with(".md") { r.clone() } else { format!("{r}.md") };
        let row = match index.note_by_path(&path)? {
            Some(row) => Some(row),
            None => index.note_by_ref(&r)?,
        };
        if let Some(row) = row {
            return Ok((p, row));
        }
    }
    Err(fail(NOT_FOUND, format!("no note {target:?} (pg ls lists notes, pg find searches them)")))
}

/// "file:40-60" -> ("file", 40, 60); "file:40" -> ("file", 40, 40).
fn lines_of(target: &str) -> Option<(&str, usize, usize)> {
    let (file, range) = target.rsplit_once(':')?;
    let (a, b) = range.split_once('-').unwrap_or((range, range));
    let (a, b) = (a.trim().parse::<usize>().ok()?, b.trim().parse::<usize>().ok()?);
    (a >= 1 && b >= a).then_some((file, a, b))
}

/// Prints code: a symbol (with the notes about it), some lines, or a whole
/// file, within the budget. None when the target is not code of the project.
fn read_code(cx: &Context, target: &str) -> Result<Option<String>> {
    let Some(project) = &cx.project else { return Ok(None) };
    let Some(code) = code::notes::open(&cx.home, project)? else { return Ok(None) };
    let (file, symbol, range) = match (target.split_once('#'), lines_of(target)) {
        (Some((f, s)), _) => (f, Some(s), None),
        (None, Some((f, a, b))) => (f, None, Some((a, b))),
        (None, None) => (target, None, None),
    };
    let Some((folder, path)) = code.file(file)? else { return Ok(None) };
    let shown = code.display(&folder, &path);
    let mut b = Budget::new(cx.budget);
    let (from, to) = match (symbol, range) {
        (Some(name), _) => {
            let symbols = code.symbols_of(&folder, &path)?;
            let Some(s) =
                symbols.iter().find(|s| s.qualified == name).or_else(|| symbols.iter().find(|s| s.name == name))
            else {
                let names: Vec<&str> = symbols.iter().map(|s| s.qualified.as_str()).take(12).collect();
                return Err(fail(NOT_FOUND, format!("no {name:?} in {shown} (it has: {})", names.join(", "))));
            };
            b.add(&format!("[{shown}#{} {} {}-{}]", s.qualified, s.kind, s.start, s.end));
            let notes = CodeNotes::load(&Index::open(project)?)?;
            for n in notes.about(&path, Some(&s.qualified)) {
                b.add(&format!("note {} {}: {}", n.id, n.kind.as_str(), crate::output::one_line(&n.text, 100)));
            }
            (s.start, s.end)
        }
        (None, Some(r)) => {
            b.add(&format!("[{shown}:{}-{}]", r.0, r.1));
            r
        }
        (None, None) => {
            b.add(&format!("[{shown}]"));
            (1, usize::MAX)
        }
    };
    print_lines(&code, &folder, &path, (from, to), &mut b, cx.budget, &shown)?;
    Ok(Some(b.out))
}

fn print_lines(
    code: &CodeIndex,
    folder: &str,
    path: &str,
    (from, to): (usize, usize),
    b: &mut Budget,
    budget: usize,
    shown: &str,
) -> Result<()> {
    let dir = code.folders.iter().find(|f| f.key == folder).map(|f| f.dir.clone()).unwrap_or_default();
    let text = files::read_text(&dir.join(path))?;
    let lines: Vec<&str> = text.lines().collect();
    let to = to.min(lines.len());
    for (i, l) in lines.iter().enumerate().take(to).skip(from.saturating_sub(1)) {
        if !b.add(&format!("{:>4}  {l}", i + 1)) {
            b.out.push_str(&format!("... more: pg read {shown}:{}-{to} --budget {}\n", i + 1, budget * 2));
            break;
        }
    }
    Ok(())
}
