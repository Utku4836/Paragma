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
    for p in &projects {
        let index = Index::open(p)?;
        hits.extend(search::find(&index, &query)?.into_iter().map(|h| (p.name.clone(), h)));
    }
    hits.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
    let mut code = if several { Vec::new() } else { secret_lines(&projects[0], &query) };
    if !several {
        code.extend(code_lines(cx, &projects[0], &query)?);
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
    let mut seen = std::collections::HashSet::new();
    for p in &projects {
        let since = crate::output::now() - STILL_IN_CONTEXT;
        if let Ok(ids) = State::open(p).and_then(|s| s.shown_in_full(since)) {
            seen.extend(ids.into_iter().map(|id| (p.name.clone(), id)));
        }
    }
    let mut shown = 0;
    let mut in_full = Vec::new();
    let mut stale: Vec<(String, crate::code::stale::Staleness)> =
        projects.iter().map(|p| (p.name.clone(), crate::code::stale::Staleness::new(&cx.home, p))).collect();
    let indexes: Vec<(String, Index)> =
        projects.iter().map(|p| Ok((p.name.clone(), Index::open(p)?))).collect::<Result<_>>()?;
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
    print!("{}", b.out);
    // what was shown, per project: what is still in the agent's context,
    // and what upkeep keeps as used (record/upkeep.rs)
    for p in &projects {
        let full: Vec<String> = in_full.iter().filter(|(n, _)| *n == p.name).map(|(_, id)| id.clone()).collect();
        let lines: Vec<String> = hits
            .iter()
            .take(shown + ONE_LINERS)
            .filter(|(name, h)| *name == p.name && !full.contains(&h.id))
            .map(|(_, h)| h.id.clone())
            .collect();
        // a search that found something never fails over bookkeeping
        if let Ok(state) = State::open(p) {
            let _ = state.shown(&full, true);
            let _ = state.shown(&lines, false);
        }
    }
    Ok(())
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
