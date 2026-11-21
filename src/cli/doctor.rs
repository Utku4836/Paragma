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

    /// What each expert said, how often it was acted on and how it weighs now; what pg did by itself
    #[arg(long, conflicts_with = "rebuild")]
    experts: bool,
}

/// What each expert said, how often the conversations acted on it and how
/// that weighs it now (kernel/adapt.rs), and what pg did by itself.
fn experts(project: &crate::project::Project) -> Result<()> {
    let state = crate::state::State::open(project)?;
    let facts = super::perceive::facts(project, &state, false)?;
    let usage = crate::kernel::adapt::usage(&facts, &facts.said_log);
    let mut said: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for s in &facts.said_log {
        *said.entry(crate::kernel::adapt::kind(&s.key).to_string()).or_default() += 1;
    }
    println!("experts of {} (what they said, the last 8 weeks):", project.name);
    if said.is_empty() {
        println!("  nothing said yet");
    }
    let about = |k: &str| match k {
        "cut" => "a conversation that stopped without a handoff",
        "so-far" => "this conversation's own work, after compaction",
        "work" => "who else is at work here",
        "recipes" => "how the project runs (its test and build commands)",
        "outdated" => "notes that may be outdated",
        "tidy" => "notes to tidy",
        "told" => "notes from other conversations (pg tell)",
        "known" => "a known fix for a failing run",
        "stuck" => "steps left open through several handoffs",
        "drafts" => "drafts waiting for a cause",
        "revive" => "an archived note read again",
        "thin-handoff" => "a handoff without what is next",
        "guard" => "a change against a rule of that code",
        _ => "",
    };
    for (kind, n) in &said {
        let learned = match usage.get(kind) {
            Some(&(s, acted)) if s >= crate::kernel::adapt::LEARN_AFTER => {
                let rate = (acted as f64 + 1.0) / (s as f64 + 2.0);
                format!("acted on {acted} of {s}; weighs ×{:.2} now", 0.5 + rate)
            }
            Some(&(s, acted)) => format!("acted on {acted} of {s}; learns after {}", crate::kernel::adapt::LEARN_AFTER),
            None => "said only (nothing to act on)".to_string(),
        };
        println!("  {kind:<9} {:<4} {learned}  {}", n, crate::output::one_line(about(kind), 50));
    }
    let auto = super::undo::undoable(&state)?;
    let all_auto = state.journal_of("auto", crate::output::now() - 30 * 86_400)?;
    let undone = all_auto.len() - auto.len();
    let count_of = |what: &str| {
        all_auto
            .iter()
            .filter(|e| matches!(&e.did, crate::state::journal::Did::Auto { what: w, .. } if w == what))
            .count()
    };
    println!(
        "what pg did by itself (30 days): {} for conversations that stopped, {}, {}, {}, {}, recipes kept {}; {} undone (pg undo lists them)",
        count(count_of("handoff") as i64, "handoff"),
        count(count_of("draft") as i64, "draft record"),
        count(count_of("flaky") as i64, "flaky test record"),
        count(count_of("rule-draft") as i64, "draft rule"),
        count(count_of("outcome") as i64, "fix outcome line"),
        count(count_of("recipe") as i64, "time"),
        undone
    );
    // the gate: what each expert may do now (kernel/gate.rs)
    use crate::kernel::gate::{EXPERTS, Level, undone_often};
    let writing: Vec<&str> =
        EXPERTS.iter().filter(|(_, l)| *l == Level::Do).map(|(n, _)| *n).filter(|n| !undone_often(&facts, n)).collect();
    let held: Vec<&str> =
        EXPERTS.iter().filter(|(n, l)| *l == Level::Do && undone_often(&facts, n)).map(|(n, _)| *n).collect();
    let off: Vec<&str> = EXPERTS.iter().filter(|(_, l)| *l == Level::Off).map(|(n, _)| *n).collect();
    println!("write by themselves: {}", writing.join(", "));
    if !held.is_empty() {
        println!("only say for now (a third or more of what they wrote was undone): {}", held.join(", "));
    }
    if !off.is_empty() {
        println!("turned off: {}", off.join(", "));
    }
    Ok(())
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let project = cx.need_project()?;
    if a.experts {
        return experts(project);
    }
    let t = std::time::Instant::now();
    // damage deep inside a file shows only when that part is read: looked
    // for here, and such a file set aside to be made again
    let mut repaired = Vec::new();
    let libs = cx.home.root.join("libs").join("libs.db");
    let meta = project.meta_dir();
    for path in [meta.join("index.db"), meta.join("code.db"), meta.join("state.db"), libs] {
        if crate::db::damaged(&path) {
            crate::db::set_aside(&path)?;
            repaired.push(path.file_name().unwrap_or_default().to_string_lossy().into_owned());
        }
    }
    let cleared = leftovers(&project.dir);
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
    if cleared > 0 {
        println!(
            "cleared: {} of writes stopped halfway (their process was killed)",
            count(cleared as i64, "temporary file")
        );
    }
    if !repaired.is_empty() {
        println!("repaired: {} damaged, set aside (.broken) and made again", repaired.join(", "));
    }
    match &code {
        Some(c) => {
            let (files, symbols) = c.counts();
            let folders: Vec<String> = c.folders.iter().map(|f| f.key.clone()).collect();
            println!("code: {}, {} in {}", count(files, "file"), count(symbols, "symbol"), folders.join(", "));
        }
        None => println!("code: no folder linked (pg open in the code's folder links it)"),
    }
    // the whole memory looked over now (what the sleep after pg finish does)
    let found = super::sleep::look(cx, project)?;
    if !found.outdated.is_empty() {
        println!("may be outdated ({}; still right: pg write --confirm <id>):", found.outdated.len());
        for (id, why) in &found.outdated {
            println!("  {id}: {}", why.trim_start_matches("may be outdated since it was written: "));
        }
    }
    if !found.tidy.is_empty() {
        println!("could be tidier ({}):", found.tidy.len());
        for t in &found.tidy {
            println!("  {t}");
        }
    }
    if without_id > 0 {
        let verb = if without_id == 1 { "is" } else { "are" };
        println!("{} without an id (written outside pg) {verb} found by path", count(without_id, "note"));
    }
    for line in crate::record::secrets::in_notes_lines(project) {
        println!("{line}");
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

/// Clears the leftovers of killed writers in every folder of the memory.
fn leftovers(dir: &std::path::Path) -> usize {
    let mut n = crate::project::files::clear_leftovers(dir);
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if e.file_type().is_ok_and(|t| t.is_dir()) && name != ".pg" && name != ".git" {
            n += leftovers(&e.path());
        }
    }
    n
}
