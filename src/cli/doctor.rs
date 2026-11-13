//! pg doctor: the state of a project's notes, its code and their indexes.

use super::Context;
use crate::index::Index;
use crate::output::count;
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// Rebuild the indexes from the note files and the code
    #[arg(long)]
    rebuild: bool,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let project = cx.need_project()?;
    let t = std::time::Instant::now();
    let mut index = Index::open(project)?;
    if a.rebuild {
        index.rebuild(project)?;
        for f in ["code.db", "code.db-wal", "code.db-shm"] {
            let _ = std::fs::remove_file(project.meta_dir().join(f));
        }
    }
    let code = crate::code::notes::open(&cx.home, project)?;
    let notes = index.count("SELECT COUNT(*) FROM notes");
    let chunks = index.count("SELECT COUNT(*) FROM chunks");
    let without_id = index.count("SELECT COUNT(*) FROM notes WHERE id IS NULL OR id = ''");
    println!(
        "ok {}: {}, {} indexed ({} ms)",
        project.name,
        count(notes, "note"),
        count(chunks, "piece"),
        t.elapsed().as_millis()
    );
    match &code {
        Some(c) => {
            let (files, symbols) = c.counts();
            let folders: Vec<String> = c.folders.iter().map(|f| f.key.clone()).collect();
            println!("code: {}, {} in {}", count(files, "file"), count(symbols, "symbol"), folders.join(", "));
        }
        None => println!("code: no folder linked (pg open in the code's folder links it)"),
    }
    if without_id > 0 {
        println!("{} without an id (written outside pg) are found by path", count(without_id, "note"));
    }
    for line in crate::record::secrets::in_notes_lines(project) {
        println!("{line}");
    }
    // notes that grew several topics
    let mut wide = Vec::new();
    for f in crate::project::scan::notes(&project.dir) {
        if f.rel == "project.md" || f.rel == "rules.md" {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(project.dir.join(&f.rel)) else { continue };
        if super::write::plain_note(&text)
            && let Some(n) = super::write::several_topics(&text)
        {
            wide.push(format!("{} ({n})", f.rel.trim_end_matches(".md")));
        }
    }
    if !wide.is_empty() {
        println!(
            "notes with several topics: {}{}: one topic per note (pg write <folder>/<topic> \"...\"), then pg rm the old one",
            wide.iter().take(8).cloned().collect::<Vec<_>>().join(", "),
            if wide.len() > 8 { format!(" (+{})", wide.len() - 8) } else { String::new() }
        );
    }
    // a link to a folder that is gone: the code map shows nothing of it
    let links = crate::state::links::Links::load(&cx.home);
    let gone: Vec<String> =
        links.folders_of(&project.name).into_iter().filter(|f| !std::path::Path::new(f).is_dir()).collect();
    if !gone.is_empty() {
        println!("linked folder gone: {} (pg link --remove --gone; the code now: pg link <folder>)", gone.join(", "));
    }
    // files that are not notes in the memory: they go to GitHub with it
    let (count_other, folders) = not_notes(&project.dir);
    if count_other > 0 {
        println!(
            "{} in the memory that are not notes (in {}): they go to GitHub with the notes; keep code and big files in a folder of their own and pg link it",
            count(count_other as i64, "file"),
            folders.join(", ")
        );
    }
    Ok(())
}

/// Files in a project's memory folder that are neither notes nor pg's own:
/// how many, and the top folders they are in.
fn not_notes(dir: &std::path::Path) -> (usize, Vec<String>) {
    let mut n = 0;
    let mut folders: Vec<String> = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, top)) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            if path.is_dir() {
                if !name.starts_with('.') {
                    stack.push((path, if top.is_empty() { format!("{name}/") } else { top.clone() }));
                }
                continue;
            }
            let ours = name.ends_with(".md") || name.ends_with(".env") || name.starts_with('.');
            if !ours {
                n += 1;
                let at = if top.is_empty() { "./".to_string() } else { top.clone() };
                if !folders.contains(&at) {
                    folders.push(at);
                }
            }
        }
    }
    folders.sort();
    folders.truncate(6);
    (n, folders)
}
