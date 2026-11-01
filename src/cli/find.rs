//! pg find: one search for everything memory holds.

use super::{Context, NOTHING_FOUND, fail};
use crate::index::Index;
use crate::output::{Budget, count, one_line};
use crate::project::Project;
use crate::search::{self, Hit};
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
    let Some(top) = hits.first().map(|h| h.1.score) else {
        return Err(fail(NOTHING_FOUND, format!("no results for {query:?}")));
    };
    hits.retain(|h| h.1.score >= top * KEEP_RATIO);

    let scope = if several { "all projects".to_string() } else { format!("project {}", projects[0].name) };
    let mut b = Budget::new(cx.budget);
    b.add(&format!("ok {} ({scope})", count(hits.len() as i64, "result")));
    let mut shown = 0;
    for (project, h) in &hits {
        let id = if several { format!("{project}:{}", h.id) } else { h.id.clone() };
        if b.add(&full(&id, h)) {
            shown += 1;
        } else {
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
    // what was shown, per project: upkeep keeps what is used (record/upkeep.rs)
    for p in &projects {
        let ids: Vec<String> = hits
            .iter()
            .take(shown + ONE_LINERS)
            .filter(|(name, _)| *name == p.name)
            .map(|(_, h)| h.id.clone())
            .collect();
        if !ids.is_empty() {
            // a search that found something never fails over bookkeeping
            let _ = State::open(p).and_then(|s| s.shown(&ids));
        }
    }
    Ok(())
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
