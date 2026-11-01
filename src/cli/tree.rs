//! pg tree: the map of the project's code, with the notes about it marked.
//!
//! ```text
//! pg tree                       folders and files, each file with its symbols
//! pg tree payments/             one folder
//! pg tree payments/refund.go    one file: every symbol, its lines and signature
//! ```
//!
//! A file with notes about it is marked: `[2 rules, 1 error]`. When the map
//! does not fit the budget it shows folders only, each with a hint.

use super::{Context, NOT_FOUND, USAGE, fail};
use crate::code::notes::{CodeNotes, marks};
use crate::code::{self, CodeIndex};
use crate::index::Index;
use crate::output::{Budget, count};
use anyhow::Result;
use std::collections::BTreeMap;

#[derive(clap::Args)]
pub struct Args {
    /// A folder or a file of the code (default: all of it)
    path: Option<String>,
}

/// Top-level symbols named per file in the map.
const NAMED: usize = 5;

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let project = cx.need_project()?;
    let Some(code) = code::notes::open(&cx.home, project)? else {
        return Err(fail(
            USAGE,
            format!("no code folder is linked to {}: run pg open in the code's folder", project.name),
        ));
    };
    let notes = CodeNotes::load(&Index::open(project)?)?;
    let given = a.path.as_deref().unwrap_or("").trim().trim_matches('/').replace('\\', "/");
    if !given.is_empty()
        && let Some((folder, path)) = code.file(&given)?
    {
        return one_file(cx, &code, &notes, &folder, &path);
    }
    let files: Vec<(String, String, String)> = code
        .files()?
        .into_iter()
        .filter(|(f, p, _)| given.is_empty() || code.display(f, p).starts_with(&format!("{given}/")))
        .collect();
    if files.is_empty() {
        return Err(fail(NOT_FOUND, format!("no code at {given:?} (pg tree shows all of it)")));
    }

    let n_symbols: i64 = if given.is_empty() {
        code.counts().1
    } else {
        files.iter().map(|(f, p, _)| code.symbols_of(f, p).map(|s| s.len() as i64).unwrap_or(0)).sum()
    };
    let mut langs: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, l) in &files {
        if !l.is_empty() {
            *langs.entry(l.as_str()).or_default() += 1;
        }
    }
    let mut langs: Vec<(&str, usize)> = langs.into_iter().collect();
    langs.sort_by_key(|l| std::cmp::Reverse(l.1));
    let head = format!(
        "ok {}{}: {}, {} ({})",
        project.name,
        if given.is_empty() { String::new() } else { format!("/{given}") },
        count(files.len() as i64, "file"),
        count(n_symbols, "symbol"),
        langs.iter().take(4).map(|(l, n)| format!("{l} {n}")).collect::<Vec<_>>().join(", ")
    );

    // the whole map, file by file
    let mut names_of = code.top_names()?;
    let mut lines = vec![head.clone()];
    let mut last_dir = String::new();
    for (f, p, _) in &files {
        let shown = code.display(f, p);
        let (dir, name) = shown.rsplit_once('/').unwrap_or(("", shown.as_str()));
        if dir != last_dir {
            lines.push(format!("{dir}/"));
            last_dir = dir.to_string();
        }
        let top = names_of.remove(&(f.clone(), p.clone())).unwrap_or_default();
        let more = top.len().saturating_sub(NAMED);
        let names = top.into_iter().take(NAMED).collect::<Vec<_>>().join(", ");
        let mark = marks(&notes.about(p, None));
        let line = format!(
            "  {name}  {names}{}{}",
            if more > 0 { format!(" (+{more})") } else { String::new() },
            if mark.is_empty() { String::new() } else { format!("  {mark}") }
        );
        lines.push(line.trim_end().to_string());
    }
    let mut b = Budget::new(cx.budget);
    if b.add(&lines.join("\n")) {
        print!("{}", b.out);
        return Ok(());
    }

    // too big: folders only
    let mut b = Budget::new(cx.budget);
    b.add(&head);
    let depth = given.matches('/').count() + if given.is_empty() { 1 } else { 2 };
    let mut dirs: BTreeMap<String, (usize, usize)> = BTreeMap::new(); // files, files with notes
    for (f, p, _) in &files {
        let shown = code.display(f, p);
        let parts: Vec<&str> = shown.split('/').collect();
        let dir = if parts.len() > depth { parts[..depth].join("/") + "/" } else { "./".to_string() };
        let e = dirs.entry(dir).or_default();
        e.0 += 1;
        e.1 += usize::from(!notes.about(p, None).is_empty());
    }
    for (dir, (n, with_notes)) in &dirs {
        let notes = if *with_notes > 0 { format!(", {with_notes} with notes") } else { String::new() };
        if !b.add(&format!("{dir}  {}{notes}", count(*n as i64, "file"))) {
            break;
        }
    }
    b.add("a folder in full: pg tree <folder>/");
    print!("{}", b.out);
    Ok(())
}

fn one_file(cx: &Context, code: &CodeIndex, notes: &CodeNotes, folder: &str, path: &str) -> Result<()> {
    let symbols = code.symbols_of(folder, path)?;
    let mut b = Budget::new(cx.budget);
    let mark = marks(&notes.about(path, None));
    b.add(&format!(
        "ok {}: {}{}",
        code.display(folder, path),
        count(symbols.len() as i64, "symbol"),
        if mark.is_empty() { String::new() } else { format!("  {mark}") }
    ));
    for s in &symbols {
        let indent = "  ".repeat(s.qualified.matches('.').count());
        let mark = marks(&notes.about(path, Some(&s.qualified)));
        let line = format!(
            "{indent}{} {}  {}-{}  {}{}",
            s.kind,
            s.name,
            s.start,
            s.end,
            s.signature,
            if mark.is_empty() { String::new() } else { format!("  {mark}") }
        );
        if !b.add(&line) {
            b.add(&format!("... more: pg tree {} --budget {}", code.display(folder, path), cx.budget * 2));
            break;
        }
    }
    print!("{}", b.out);
    Ok(())
}
