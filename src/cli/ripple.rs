//! pg ripple: what a change reaches.
//!
//! ```text
//! pg ripple                    what changed since the last commit, and what it reaches:
//!                              callers not updated, the tests to run
//! pg ripple Refund             what calls it and tests it, and the notes about it
//! pg ripple payments/refund.go#Refund
//! ```
//!
//! The same is said by itself where a change may break something: in
//! pg run, the hooks after an edit, and pg finish (code/ripple.rs).

use super::{Context, NOT_FOUND, fail};
use crate::code::notes::CodeNotes;
use crate::code::ripple::{self, How};
use crate::code::{CodeIndex, Sym};
use crate::index::Index;
use crate::output::Budget;
use crate::state::State;
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// A function, method or type (Refund, Cart.Total, payments/refund.go#Refund); without one: what changed since the last commit
    target: Option<String>,
}

/// Callers listed per symbol.
const CALLERS: usize = 6;

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let project = cx.need_project()?;
    crate::code::git::changed_ahead(cx.code_dirs(project));
    let Some(code) = crate::code::notes::open(&cx.home, project)? else {
        return Err(fail(NOT_FOUND, "no code folder linked to this project: pg open in the code's folder"));
    };
    let mut b = Budget::new(cx.budget);
    match a.target.as_deref() {
        None => {
            let all = ripple::of_changes(&code, None)?;
            let (shape, body): (Vec<_>, Vec<_>) = all.iter().partition(|r| !matches!(r.how, How::Body));
            if shape.is_empty() && body.is_empty() {
                b.add("ok ripple: nothing changed since the last commit reaches other code (about one: pg ripple <function>)");
            } else {
                let breaking = shape.iter().filter(|r| r.breaks()).count();
                b.add(&format!(
                    "ok ripple: {} changed in shape ({breaking} with callers not updated), {} inside only",
                    shape.len(),
                    body.len()
                ));
                for r in &shape {
                    for l in ripple::show(&code, r, CALLERS) {
                        b.add(&l);
                    }
                }
                // changed inside only: their tests are what to run
                let tested: Vec<String> = body
                    .iter()
                    .filter(|r| !r.tests.is_empty())
                    .map(|r| {
                        format!(
                            "{} ({})",
                            r.qualified,
                            r.tests.iter().take(2).map(|t| t.qualified.clone()).collect::<Vec<_>>().join(", ")
                        )
                    })
                    .collect();
                if !tested.is_empty() {
                    b.add(&format!("changed inside, with tests: {}", tested.join("; ")));
                }
                let untested: Vec<&str> =
                    body.iter().filter(|r| r.tests.is_empty()).map(|r| r.qualified.as_str()).collect();
                if !untested.is_empty() {
                    b.add(&format!("changed inside, no test reaches them: {}", untested.join(", ")));
                }
            }
        }
        Some(t) => {
            let found = symbols(&code, t)?;
            if found.is_empty() {
                return Err(fail(
                    NOT_FOUND,
                    format!("no function, method or type {t:?} in the code (pg tree lists them)"),
                ));
            }
            b.add(&format!(
                "ok ripple: {}",
                if found.len() == 1 { "1 symbol".into() } else { format!("{} symbols named {t}", found.len()) }
            ));
            let notes = CodeNotes::load(&Index::open(project)?)?;
            for s in found.iter().take(3) {
                let r = ripple::of_symbol(&code, s)?;
                for l in ripple::show(&code, &r, CALLERS) {
                    b.add(&l);
                }
                if r.callers.is_empty() {
                    b.add("  nothing here calls it");
                }
                for n in notes.about(&s.path, Some(&s.qualified)).into_iter().take(4) {
                    b.add(&format!("  note {} {}: {}", n.id, n.kind.as_str(), crate::output::one_line(&n.text, 90)));
                }
            }
        }
    }
    print!("{}", b.out);
    Ok(())
}

/// The symbols a target names: file#Symbol, a qualified name or a name.
fn symbols(code: &CodeIndex, target: &str) -> Result<Vec<Sym>> {
    if let Some((file, sym)) = target.split_once('#') {
        let Some((folder, path)) = code.file(file)? else { return Ok(Vec::new()) };
        return Ok(code
            .symbols_of(&folder, &path)?
            .into_iter()
            .filter(|s| s.qualified == sym || s.name == sym)
            .collect());
    }
    Ok(code.named(target)?.into_iter().filter(|s| s.kind != "section").collect())
}

/// For pg run, the hooks and pg finish: one line per change that leaves
/// callers as they were, each said once in a conversation (`always`: also
/// when said before). `only`: the files just edited.
pub fn warnings(
    project: &crate::project::Project,
    code: &CodeIndex,
    only: Option<&[(String, String)]>,
    always: bool,
) -> Vec<String> {
    let Ok(all) = ripple::of_changes(code, only) else { return Vec::new() };
    let state = State::open(project).ok();
    let mut out = Vec::new();
    for r in all.iter().filter(|r| r.breaks()) {
        let Some(line) = ripple::warning(code, r) else { continue };
        let key = format!(
            "ripple:{}:{}:{}",
            crate::state::conversation::key(),
            r.qualified,
            crate::code::parse::hash(&format!("{:?}", r.how))
        );
        if let Some(s) = &state {
            if !always && s.get(&key).is_some() {
                continue;
            }
            let _ = s.set(&key, "told");
        }
        out.push(line);
        if out.len() >= 5 {
            break;
        }
    }
    out
}
