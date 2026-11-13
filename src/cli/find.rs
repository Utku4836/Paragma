//! pg find: one search for everything memory holds. A code name or a
//! file of the project's code also finds the code itself, first, with the
//! notes about it. What this conversation was shown in full lately comes as
//! one line: it is still in the agent's context.

use super::{Context, NOTHING_FOUND, fail};
use crate::code::lang::Lang;
use crate::code::notes::{CodeNotes, marks};
use crate::index::Index;
use crate::output::{Budget, count, one_line};
use crate::project::Project;
use crate::search::{self, Hit, QueryKind};
use crate::state::State;
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// What to look for: a topic, a question, an error line, a file or a code name
    #[arg(required = true)]
    query: Vec<String>,

    /// Search another project
    #[arg(long = "in", value_name = "PROJECT")]
    in_project: Option<String>,

    /// Search every project
    #[arg(long)]
    all: bool,
}

/// Results below this share of the best score are not shown.
const KEEP_RATIO: f64 = 0.45;
/// Results shown as one line after the budget for full text is spent.
const ONE_LINERS: usize = 5;
/// A result shown in full to the conversation this recently is not shown
/// in full again.
const STILL_IN_CONTEXT: i64 = 90 * 60;

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let query = a.query.join(" ");
    let projects: Vec<Project> = match (&a.in_project, a.all, &cx.project) {
        (Some(name), _, _) => {
            vec![cx.home.project(name).ok_or_else(|| fail(super::NOT_FOUND, format!("no project {name:?}")))?]
        }
        (None, false, Some(p)) => vec![p.clone()],
        _ => cx.home.projects(),
    };
    let several = projects.len() > 1;
    let mut hits: Vec<(String, Hit)> = Vec::new();
    // each project's state, opened once: outcomes now, what was shown below
    let states: Vec<Option<State>> = projects.iter().map(|p| State::open(p).ok()).collect();
    let indexes: Vec<(String, Index)> =
        projects.iter().map(|p| Ok((p.name.clone(), Index::open(p)?))).collect::<Result<_>>()?;
    for ((p, state), (_, index)) in projects.iter().zip(&states).zip(&indexes) {
        let mut found = search::find(index, &query)?;
        if let Some(state) = state {
            search::weigh_outcomes(&mut found, state);
        }
        hits.extend(found.into_iter().map(|h| (p.name.clone(), h)));
    }
    hits.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
    crate::output::mark("find: notes searched");
    let mut code = if several { Vec::new() } else { secret_lines(&projects[0], &query) };
    if !several {
        code.extend(code_lines(cx, &projects[0], &query)?);
        crate::output::mark("find: code lines");
        code.extend(doc_lines(cx, &projects[0], &query));
        crate::output::mark("find: doc lines");
        code.extend(lib_lines(cx, &projects[0], &query));
        crate::output::mark("find: libraries");
    }
    if hits.is_empty() && code.is_empty() {
        return Err(fail(NOTHING_FOUND, format!("no results for {query:?}")));
    }
    let top = hits.first().map(|h| h.1.score).unwrap_or(0.0);
    hits.retain(|h| h.1.score >= top * KEEP_RATIO);

    let scope = if several { "all projects".to_string() } else { format!("project {}", projects[0].name) };
    let mut b = Budget::new(cx.budget);
    b.add(&format!("ok {} ({scope})", count((hits.len() + code.len()) as i64, "result")));
    for line in &code {
        b.add(line);
    }
    // shown in full lately and not changed since: still in the agent's
    // context. A note edited or a record updated since is shown again.
    let mut seen = std::collections::HashSet::new();
    let mut versions = std::collections::HashMap::new();
    for ((p, state), (_, index)) in projects.iter().zip(&states).zip(&indexes) {
        let refs = hits.iter().filter(|(name, _)| *name == p.name).map(|(_, h)| h.id.as_str());
        let now = index.versions(refs).unwrap_or_default();
        let since = crate::output::now() - STILL_IN_CONTEXT;
        if let Some(saw) = state.as_ref().and_then(|s| s.seen_in_full(since).ok()) {
            for (id, v) in &now {
                if saw.get(id) == Some(v) {
                    seen.insert((p.name.clone(), id.clone()));
                }
            }
        }
        versions.insert(p.name.clone(), now);
    }
    let mut shown = 0;
    let mut in_full = Vec::new();
    let mut stale: Vec<(String, crate::code::stale::Staleness)> =
        projects.iter().map(|p| (p.name.clone(), crate::code::stale::Staleness::new(&cx.home, p))).collect();
    for (project, h) in &hits {
        let id = if several { format!("{project}:{}", h.id) } else { h.id.clone() };
        let block = if seen.contains(&(project.clone(), h.id.clone())) {
            format!("{id} {} {}: {} (shown above; pg read {id})", h.kind.as_str(), where_(h), one_line(&h.text, 70))
        } else {
            in_full.push((project.clone(), h.id.clone()));
            let props = indexes.iter().find(|i| i.0 == *project).map(|i| i.1.props_at(&h.path)).unwrap_or_default();
            let warning = match stale.iter_mut().find(|s| s.0 == *project) {
                Some((_, s)) => s.check(&props)?,
                None => None,
            };
            match warning {
                Some(w) => full(&id, h).replacen('\n', &format!("\n({w})\n"), 1),
                None => full(&id, h),
            }
        };
        if b.add(&block) {
            shown += 1;
        } else {
            in_full.pop();
            break;
        }
    }
    let rest = &hits[shown..];
    for (project, h) in rest.iter().take(ONE_LINERS) {
        let id = if several { format!("{project}:{}", h.id) } else { h.id.clone() };
        b.add(&format!("{id} {} {}{}: {}", h.kind.as_str(), where_(h), similar(h), one_line(&h.text, 90)));
    }
    if rest.len() > ONE_LINERS {
        b.add(&format!("+{} more: pg find \"{query}\" --budget {}", rest.len() - ONE_LINERS, cx.budget * 2));
    }
    // what the best result is connected to, beyond these results
    if !several && let Some((_, best)) = hits.first() {
        let listed: Vec<String> = hits.iter().map(|(_, h)| h.id.clone()).collect();
        let links = crate::search::graph::connected_by(&projects[0], &indexes[0].1, &best.id, 2, &listed, false)?;
        if let Some(l) = crate::search::graph::line(&links) {
            b.add(&l.replacen("connected:", &format!("{} is connected to:", best.id), 1));
        }
    }
    print!("{}", b.out);
    // what was shown, per project: what is still in the agent's context,
    // and what upkeep keeps as used (record/upkeep.rs)
    for (p, state) in projects.iter().zip(&states) {
        let version = |id: &String| versions.get(&p.name).and_then(|v| v.get(id)).copied().unwrap_or(0);
        let full: Vec<(String, i64)> =
            in_full.iter().filter(|(n, _)| *n == p.name).map(|(_, id)| (id.clone(), version(id))).collect();
        let lines: Vec<String> = hits
            .iter()
            .take(shown + ONE_LINERS)
            .filter(|(name, h)| *name == p.name && !full.iter().any(|(id, _)| *id == h.id))
            .map(|(_, h)| h.id.clone())
            .collect();
        // a search that found something never fails over bookkeeping
        if let Some(state) = state {
            let _ = state.shown_in_full(&full);
            let _ = state.shown(&lines);
        }
    }
    Ok(())
}

/// Library items the query names (`tree_sitter Parser new`), at the
/// version the project uses: the first ones in full.
fn lib_lines(cx: &Context, project: &Project, query: &str) -> Vec<String> {
    let dirs = cx.code_dirs(project);
    let Ok(found) = crate::libs::find(&cx.home, &dirs, query, LIB_ITEMS) else { return Vec::new() };
    found.iter().enumerate().map(|(i, f)| crate::libs::show(f, i >= LIB_IN_FULL)).collect()
}

/// Library items listed, and how many of them in full.
const LIB_ITEMS: usize = 6;
const LIB_IN_FULL: usize = 2;

/// How close in meaning a section of the project's documents must be to
/// be listed.
const DOC_MIN: f32 = 0.45;
/// Sections listed.
const DOCS: usize = 3;
/// What a query word in a section's heading adds.
const DOC_BY_NAME: f32 = 0.15;
/// Sections this far below the best one are left out.
const DOC_MARGIN: f32 = 0.06;

/// The sections of the project's Markdown files (README, docs/) a query is
/// about, by meaning: where to read, not the text.
fn doc_lines(cx: &Context, project: &Project, query: &str) -> Vec<String> {
    if search::classify(query) == QueryKind::Code || query.split_whitespace().count() < 2 {
        return Vec::new();
    }
    let Ok(Some(code)) = crate::code::CodeIndex::open_as_is(&cx.home, project) else { return Vec::new() };
    docs_about(&code, query, DOCS)
        .into_iter()
        .map(|s| {
            format!(
                "[doc] {}#{} {}-{}{}",
                code.display(&s.folder, &s.path),
                s.qualified,
                s.start,
                s.end,
                if s.doc.is_empty() { String::new() } else { format!(": {}", one_line(&s.doc, 90)) }
            )
        })
        .collect()
}

/// The sections of the project's documents closest to a text: by meaning,
/// and a little for a word of it in the heading.
pub(super) fn docs_about(code: &crate::code::CodeIndex, query: &str, n: usize) -> Vec<crate::code::Sym> {
    let Ok(mut near) = code.nearest_docs(query, 30) else { return Vec::new() };
    // a word of the query in the heading counts too ("folders" for "Folders")
    let words: Vec<String> = search::words_of(query).into_iter().filter(|w| w.chars().count() >= 4).collect();
    for (s, sim) in near.iter_mut() {
        let heading: Vec<String> = search::words_of(&s.name);
        if words.iter().any(|w| {
            heading.iter().any(|h| h.starts_with(w.as_str()) || w.starts_with(h.as_str()) && h.chars().count() >= 4)
        }) {
            *sim += DOC_BY_NAME;
        }
    }
    near.sort_by(|a, b| b.1.total_cmp(&a.1));
    if std::env::var_os("PG_EXPLAIN").is_some() {
        for (s, sim) in near.iter().take(6) {
            eprintln!("explain: doc {}#{} {sim:.3}", s.path, s.qualified);
        }
    }
    let top = near.first().map(|n| n.1).unwrap_or(0.0);
    near.into_iter().filter(|(_, sim)| *sim >= DOC_MIN && *sim >= top - DOC_MARGIN).take(n).map(|(s, _)| s).collect()
}

/// The project's secrets a query asks for, by title; never their values.
fn secret_lines(project: &Project, query: &str) -> Vec<String> {
    let secrets = crate::project::env::load(project);
    crate::project::env::matching(&secrets, query)
        .iter()
        .take(10)
        .map(|s| format!("[secret] {}: use ${} in pg run", s.show(), s.name))
        .collect()
}

/// The project's code a query names: a file, file#Symbol, or a symbol
/// ("Refund", "Cart.Total"), each with the notes about it.
fn code_lines(cx: &Context, project: &Project, query: &str) -> Result<Vec<String>> {
    let q = query.trim();
    let single_name = !q.contains(' ')
        && q.chars().next().is_some_and(char::is_uppercase)
        && q.chars().all(|c| c.is_alphanumeric() || c == '.');
    let codey = search::classify(q) == QueryKind::Code || Lang::of(q).is_some() || single_name;
    if !codey || q.split_whitespace().count() > 2 {
        return Ok(Vec::new());
    }
    let Some(code) = crate::code::notes::open(&cx.home, project)? else { return Ok(Vec::new()) };
    let notes = CodeNotes::load(&Index::open(project)?)?;
    let (file, symbol) = match q.split_once('#') {
        Some((f, s)) => (Some(f), Some(s)),
        None if q.contains('/') || Lang::of(q).is_some() => (Some(q), None),
        None => (None, Some(q)),
    };
    let mut symbols = Vec::new();
    if let Some(f) = file {
        let Some((folder, path)) = code.file(f)? else { return Ok(Vec::new()) };
        match symbol {
            Some(s) => {
                symbols.extend(code.symbols_of(&folder, &path)?.into_iter().filter(|x| x.qualified == s || x.name == s))
            }
            None => {
                let n = code.symbols_of(&folder, &path)?.len();
                let mark = marks(&notes.about(&path, None));
                return Ok(vec![
                    format!("[code] {}: {}  {mark}", code.display(&folder, &path), count(n as i64, "symbol"))
                        .trim_end()
                        .to_string(),
                ]);
            }
        }
    } else if let Some(s) = symbol {
        symbols = code.named(s)?;
    }
    Ok(symbols
        .iter()
        .take(3)
        .map(|s| {
            let mark = marks(&notes.about(&s.path, Some(&s.qualified)));
            format!(
                "[code] {}#{} {} {}-{}: {}  {mark}",
                code.display(&s.folder, &s.path),
                s.qualified,
                s.kind,
                s.start,
                s.end,
                if s.doc.is_empty() { &s.signature } else { &s.doc }
            )
            .trim_end()
            .to_string()
        })
        .collect())
}

fn where_(h: &Hit) -> String {
    let path = h.path.trim_end_matches(".md");
    if h.heading.is_empty() { path.to_string() } else { format!("{path}#{}", h.heading) }
}

fn full(id: &str, h: &Hit) -> String {
    format!("[{id} {}] {}{}\n{}\n", h.kind.as_str(), where_(h), similar(h), h.text.trim())
}

/// "  (+3 similar)" when near duplicates were folded into the result.
fn similar(h: &Hit) -> String {
    if h.similar == 0 { String::new() } else { format!("  (+{} similar)", h.similar) }
}
