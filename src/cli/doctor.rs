//! pg doctor: the state of a project's notes and index.

use super::Context;
use crate::index::Index;
use crate::output::count;
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// Rebuild the index from the note files
    #[arg(long)]
    rebuild: bool,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let project = cx.need_project()?;
    let t = std::time::Instant::now();
    let mut index = Index::open(project)?;
    if a.rebuild {
        index.rebuild(project)?;
    }
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
    if without_id > 0 {
        println!("{} without an id (written outside pg) are found by path", count(without_id, "note"));
    }
    Ok(())
}
