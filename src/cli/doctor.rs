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
    Ok(())
}
