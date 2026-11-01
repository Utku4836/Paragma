//! pg open: start or continue the work on a project, and get its brief.
//!
//! ```text
//! pg open shop --goal "Refunds through the new provider" --step "Add the client"
//! pg open                 the project linked to this folder
//! ```
//!
//! The folder it runs in is linked to the project, so later commands here
//! (and a new conversation's `pg open`) know the project without its name.
//! The brief gathers what an agent would otherwise look up in five to ten
//! calls: the goal, the open steps, the last handoff, what other
//! conversations are doing, and the rules, decisions and errors that match
//! the goal and the steps.

use super::{Context, USAGE, fail};
use crate::code::{CodeIndex, git, work};
use crate::index::{Index, update_note};
use crate::note::{Kind, Note};
use crate::output::{Budget, ago, count, now, one_line};
use crate::project::names::{self, Match};
use crate::project::{Project, files, plan};
use crate::search;
use crate::state::State;
use crate::state::links::Links;
use crate::testrun;
use anyhow::Result;
use std::path::Path;

#[derive(clap::Args)]
pub struct Args {
    /// The project (default: the one linked to this folder)
    name: Vec<String>,

    /// What the work is for (replaces the goal)
    #[arg(long)]
    goal: Option<String>,

    /// A step of the work (repeat for more)
    #[arg(long = "step", value_name = "STEP")]
    steps: Vec<String>,

    /// Start a new project even when the name is close to an existing one
    #[arg(long)]
    new: bool,
}

/// How the name given led to the project.
enum How {
    New { similar: Vec<String> },
    Continued,
    SameName(String),
    Close(String),
    OfThisFolder,
}

/// Conversations older than this are not mentioned in the brief.
const RECENT: i64 = 14 * 86_400;
/// Open steps listed in full.
const STEPS_SHOWN: usize = 10;
/// Handoffs before the last one, listed as one line each.
const EARLIER_HANDOFFS: usize = 2;
/// Files listed as likely to be touched, or as changed.
const LIKELY: usize = 5;
/// How close in meaning (plus name) code must be to the goal to be listed.
const LIKELY_MIN: f32 = 0.35;
/// Rules, decisions and errors listed as matching the goal and the steps.
const RELEVANT: usize = 8;

pub fn run(cx: &Context, mut a: Args) -> Result<()> {
    let warning = super::mask_all(a.goal.iter_mut().chain(a.steps.iter_mut()));
    let names: Vec<String> = cx.home.projects().into_iter().map(|p| p.name).collect();
    let links = Links::load(&cx.home);
    let linked: Vec<String> = links.projects_of(&cx.cwd).into_iter().filter(|p| names.contains(p)).collect();
    // what an agent may call the project of this folder: the folder's name
    let mut folders = links.folder_names(&cx.cwd);
    folders.extend(cx.cwd.file_name().map(|f| f.to_string_lossy().into_owned()));
    let folders: Vec<String> = folders.iter().map(|f| names::slug(f)).collect();
    let given = a.name.join(" ").trim().to_string();
    let (name, how) = choose(cx, &given, &names, &linked, &folders, a.new)?;
    let project = match cx.home.project(&name) {
        Some(p) => p,
        None => cx.home.create_project(&name).map_err(|e| fail(USAGE, e.to_string()))?,
    };

    let state = State::open(&project)?;
    let has_note = project.note_file(plan::FILE).exists();
    if !has_note || a.goal.is_some() || !a.steps.is_empty() {
        let (_, added) = update_note(&project, plan::FILE, |n, exists, _| {
            if !exists {
                n.props.set_text("type", Kind::Project.as_str());
                n.props.set_text("title", project.name.clone());
            }
            if let Some(g) = &a.goal {
                plan::set_goal(n, g);
            }
            Ok(a.steps.iter().filter(|s| plan::add_step(n, s)).cloned().collect::<Vec<_>>())
        })?;
        for s in added {
            state.event("step", &s, "")?;
        }
    }
    if linkable(cx) {
        Links::link(&cx.home, &cx.cwd, &project.name)?;
    }
    state.touch_session(&work_folder(cx, &project.name))?;
    state.event("open", &project.name, "")?;

    let others: Vec<String> = linked.into_iter().filter(|p| *p != project.name).collect();
    print!("{}", brief(cx, &project, &state, &first_line(&project.name, &how), &how, &others)?);
    if let Some(w) = warning {
        println!("{w}");
    }
    Ok(())
}

/// The project a name means (see project/names.rs).
fn choose(
    cx: &Context,
    given: &str,
    names: &[String],
    linked: &[String],
    folders: &[String],
    new: bool,
) -> Result<(String, How)> {
    if given.is_empty() {
        return match &cx.project {
            Some(p) => Ok((p.name.clone(), How::Continued)),
            None if names.is_empty() => Err(fail(USAGE, "no projects yet: pg open <name> starts one")),
            None => Err(fail(
                USAGE,
                format!(
                    "no project is linked to this folder: pg open <name> starts or continues one (projects: {})",
                    names.join(", ")
                ),
            )),
        };
    }
    let slug = names::slug(given);
    if slug.is_empty() {
        return Err(fail(USAGE, format!("{given:?} cannot be a project name: use letters and digits")));
    }
    if new {
        if names.contains(&slug) {
            return Err(fail(USAGE, format!("project {slug} exists: pg open {slug} continues it")));
        }
        return Ok((slug, How::New { similar: Vec::new() }));
    }
    match names::match_name(given, names) {
        Match::Same(e) if e == given => Ok((e, How::Continued)),
        Match::Same(e) => Ok((e, How::SameName(given.to_string()))),
        _ if (names::GENERIC.contains(&slug.as_str()) || folders.contains(&slug)) && !linked.is_empty() => {
            Ok((linked[0].clone(), How::OfThisFolder))
        }
        Match::Close(e) => Ok((e, How::Close(given.to_string()))),
        Match::Similar(similar) => Ok((slug, How::New { similar })),
        Match::None => Ok((slug, How::New { similar: Vec::new() })),
    }
}

fn first_line(name: &str, how: &How) -> String {
    match how {
        How::New { .. } => format!("ok {name} (new project)"),
        How::Continued => format!("ok {name} (continued)"),
        How::SameName(g) => format!("ok {name} (continued: same name as {g:?})"),
        How::Close(g) => format!("ok {name} (continued: close to {g:?}; a new project instead: pg open {g:?} --new)"),
        How::OfThisFolder => format!("ok {name} (continued: the project of this folder)"),
    }
}

/// Whether to link the folder pg runs in: not the home folder, a drive's
/// root or Paragma's own folder, which would link everything below them.
fn linkable(cx: &Context) -> bool {
    let home = std::env::home_dir();
    cx.cwd.parent().is_some() && home.as_deref() != Some(cx.cwd.as_path()) && !cx.cwd.starts_with(&cx.home.root)
}

fn brief(cx: &Context, project: &Project, state: &State, first: &str, how: &How, others: &[String]) -> Result<String> {
    let mut b = Budget::new(cx.budget);
    b.add(first);
    if let How::New { similar } = how {
        for s in similar.iter().take(3) {
            let open = open_steps(cx.home.project(s).as_ref());
            b.add(&format!("similar project: {s} ({}); if it is the same work: pg open {s}", count(open, "open step")));
        }
    }
    if !others.is_empty() {
        b.add(&format!("this folder is also linked to: {}", others.join(", ")));
    }

    let note = files::read_text(&project.note_file(plan::FILE)).map(|t| Note::parse(&t)).unwrap_or_default();
    let goal = plan::goal(&note);
    let steps = plan::steps(&note);
    let open: Vec<&plan::Step> = steps.iter().filter(|s| !s.done).collect();
    if goal.is_empty() {
        b.add("no goal yet: pg open --goal \"...\" --step \"...\"");
    } else {
        b.add(&format!("goal: {goal}"));
    }
    if !steps.is_empty() {
        b.add(&format!("steps: {} open, {} done", open.len(), steps.len() - open.len()));
        for s in open.iter().take(STEPS_SHOWN) {
            b.add(&format!("- [ ] {}", s.text));
        }
        if open.len() > STEPS_SHOWN {
            b.add(&format!("+{} more: pg read {}/project", open.len() - STEPS_SHOWN, project.name));
        }
    }
    let handoffs = plan::handoffs(&note);
    if let Some(h) = handoffs.first() {
        b.add(&format!("last handoff ({}):\n{}", h.heading, h.text));
    }
    for h in handoffs.iter().skip(1).take(EARLIER_HANDOFFS) {
        b.add(&format!("earlier ({}): {}", h.heading, one_line(&h.text, 90)));
    }
    let now = now();
    if let Some(line) = since_last(cx, project, state, now)? {
        b.add(&line);
    }
    let working: Vec<String> = state
        .other_sessions(now - RECENT)?
        .iter()
        .filter(|s| !s.finished)
        .take(3)
        .map(|s| {
            let folder = Path::new(&s.folder).file_name().map(|f| f.to_string_lossy().into_owned());
            format!("{} ({})", folder.unwrap_or_else(|| "?".into()), ago(s.last, now))
        })
        .collect();
    if !working.is_empty() {
        b.add(&format!("other conversations at work, not finished: {}", working.join(", ")));
    }

    let index = Index::open(project)?;
    let query: String =
        std::iter::once(goal.as_str()).chain(open.iter().map(|s| s.text.as_str())).collect::<Vec<_>>().join(". ");
    if !query.trim().is_empty() {
        let hits: Vec<search::Hit> = search::find(&index, &query)?
            .into_iter()
            .filter(|h| matches!(h.kind, Kind::Rule | Kind::Decision | Kind::Error))
            .take(RELEVANT)
            .collect();
        if !hits.is_empty() {
            b.add("matching the goal and steps:");
            let mut stale = crate::code::stale::Staleness::new(&cx.home, project);
            for h in &hits {
                let changed =
                    if stale.check(&index.props_at(&h.path))?.is_some() { " (its code changed since)" } else { "" };
                let place =
                    if h.heading.is_empty() { h.path.trim_end_matches(".md").to_string() } else { h.heading.clone() };
                b.add(&format!("{} {} {place}: {}{changed}", h.id, h.kind.as_str(), one_line(&h.text, 100)));
            }
            state.shown(&hits.iter().map(|h| h.id.clone()).collect::<Vec<_>>(), false)?;
        }
    }
    if let Some(code) = crate::code::notes::open(&cx.home, project)? {
        code_part(&mut b, &code, state, &query)?;
    }
    if let Some(s) = crate::project::env::summary(project) {
        b.add(&s);
    }
    let rules = index.count("SELECT COUNT(*) FROM chunks WHERE kind = 'rule'");
    let of = |k: &str| index.count(&format!("SELECT COUNT(*) FROM notes WHERE kind = '{k}'"));
    b.add(&format!(
        "memory: {}, {}, {}, {} (pg find <topic>, pg ls)",
        count(rules, "rule"),
        count(of("decision"), "decision"),
        count(of("error"), "error"),
        count(of("note"), "note")
    ));
    Ok(b.out)
}

/// Code lines of the brief: how the tests run (or how they last went),
/// what is changed and not committed, and the code the goal and steps are
/// likely about, with the tests that broke before when it changed.
/// "since the last handoff (2 h ago): 3 commits (fix refund window; ...),
/// files: a, b (+4); new in memory: r7, e12": what moved while no
/// conversation was here.
fn since_last(cx: &Context, project: &Project, state: &State, now: i64) -> Result<Option<String>> {
    let Some(at) = state.last_finish() else { return Ok(None) };
    let mut parts = Vec::new();
    let (mut subjects, mut files) = (Vec::new(), Vec::new());
    for f in Links::load(&cx.home).folders_of(&project.name) {
        let (s, fs) = git::since(Path::new(&f), at);
        subjects.extend(s);
        files.extend(fs);
    }
    if !subjects.is_empty() {
        let shown: Vec<String> = subjects.iter().take(3).map(|s| one_line(s, 50)).collect();
        let more = if subjects.len() > 3 { "; ..." } else { "" };
        parts.push(format!("{} ({}{more})", count(subjects.len() as i64, "commit"), shown.join("; ")));
    }
    if !files.is_empty() {
        let more = files.len().saturating_sub(5);
        let shown = files.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
        parts.push(format!("files: {shown}{}", if more > 0 { format!(" (+{more})") } else { String::new() }));
    }
    let written = state.written_since(at)?;
    if !written.is_empty() {
        parts.push(format!("new in memory: {}", written.iter().take(8).cloned().collect::<Vec<_>>().join(", ")));
    }
    Ok((!parts.is_empty()).then(|| format!("since the last handoff ({}): {}", ago(at, now), parts.join("; "))))
}

fn code_part(b: &mut Budget, code: &CodeIndex, state: &State, query: &str) -> Result<()> {
    let stack: Vec<String> = code.folders.iter().flat_map(|f| work::stack(&f.dir)).collect();
    if !stack.is_empty() {
        b.add(&format!("stack: {}", stack.join("; ")));
    }
    let used = state.commands(4)?;
    if !used.is_empty() {
        let list: Vec<String> = used
            .iter()
            .map(|(c, n, exit)| format!("{c} ({n}x, last {})", if *exit == 0 { "passed" } else { "failed" }))
            .collect();
        b.add(&format!("run here before: {}", list.join(", ")));
    }
    // the last run that ran tests (not a script or an API call)
    let last_tests = state.runs(None, 30)?.into_iter().find(|r| r.passed > 0 || !r.failing.is_empty());
    match last_tests {
        Some(r) => b.add(&format!("tests: {} (again: pg run -- {})", testrun::describe(&r), r.command)),
        None => match code.folders.iter().find_map(|f| work::test_command(&f.dir)) {
            Some(cmd) => b.add(&format!("tests: pg run -- {cmd}")),
            None => true,
        },
    };
    let changes = work::changed(code)?;
    if !changes.is_empty() {
        let more = changes.len().saturating_sub(LIKELY);
        let shown: Vec<String> = changes.iter().take(LIKELY).map(|c| c.show(code)).collect();
        b.add(&format!(
            "not committed: {}{}",
            shown.join(", "),
            if more > 0 { format!(" (+{more})") } else { String::new() }
        ));
    }
    if query.trim().is_empty() {
        return Ok(());
    }
    // by meaning, by name, and a little for what is being changed now
    let words: Vec<String> = crate::search::words_of(query).into_iter().filter(|w| w.chars().count() >= 4).collect();
    let mut best: Vec<(String, String, String, f32)> = Vec::new(); // folder, path, symbol, score
    for (s, sim) in code.nearest(query, 40)? {
        let name = s.name.to_lowercase();
        let by_name = if words.iter().any(|w| name.contains(w.as_str())) { 0.25 } else { 0.0 };
        let changing = if changes.iter().any(|c| c.path == s.path) { 0.1 } else { 0.0 };
        let score = sim + by_name + changing;
        match best.iter_mut().find(|e| e.0 == s.folder && e.1 == s.path) {
            Some(e) if e.3 >= score => {}
            Some(e) => *e = (s.folder.clone(), s.path.clone(), s.qualified.clone(), score),
            None => best.push((s.folder.clone(), s.path.clone(), s.qualified.clone(), score)),
        }
    }
    best.sort_by(|a, b| b.3.total_cmp(&a.3));
    let top = best.first().map(|e| e.3).unwrap_or(0.0);
    best.retain(|e| e.3 >= LIKELY_MIN && e.3 >= top * 0.8);
    if best.is_empty() {
        return Ok(());
    }
    let shown: Vec<String> =
        best.iter().take(LIKELY).map(|(f, p, s, _)| format!("{}#{s}", code.display(f, p))).collect();
    b.add(&format!("likely code: {}", shown.join(", ")));
    let first = best.first().map(|(f, p, _, _)| (code.folders.iter().find(|x| x.key == *f), p.as_str()));
    if let Some((Some(folder), path)) = first
        && let Some(cmd) = work::test_command(&folder.dir)
        && let Some(narrow) = work::narrow_test(cmd, &folder.dir, path)
    {
        b.add(&format!("its tests first: pg run -- {narrow}"));
    }
    let files: Vec<String> = best.iter().take(LIKELY).map(|(f, p, _, _)| code.display(f, p)).collect();
    let broke = testrun::broken_by(&state.runs(None, 100)?, &files);
    if !broke.is_empty() {
        b.add(&format!("tests that broke before when it changed: {}", broke.join(", ")));
    }
    if let Some((folder, path, _, _)) = best.first()
        && let Some(f) = code.folders.iter().find(|f| f.key == *folder)
        && let Some(with) = git::changed_together(&f.dir, 300).remove(path)
    {
        let with: Vec<String> = with.into_iter().take(3).map(|(p, _)| p).collect();
        b.add(&format!("{} usually changes with: {}", code.display(folder, path), with.join(", ")));
    }
    Ok(())
}

/// Where the project's code is: the linked folder at or above this one,
/// else this folder.
pub fn work_folder(cx: &Context, project: &str) -> String {
    Links::load(&cx.home).code_folder(&cx.cwd, project).unwrap_or_else(|| cx.cwd.to_string_lossy().into_owned())
}

fn open_steps(project: Option<&Project>) -> i64 {
    project
        .and_then(|p| files::read_text(&p.note_file(plan::FILE)).ok())
        .map(|t| plan::steps(&Note::parse(&t)).iter().filter(|s| !s.done).count() as i64)
        .unwrap_or(0)
}
