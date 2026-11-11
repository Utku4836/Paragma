//! Memory keeping itself in order, once a day at most, at `pg finish`:
//! - errors and plain notes nobody has seen, shown or changed for three
//!   months are archived (status: archived). Nothing is deleted: they rank
//!   lower and still come up when a search matches them well;
//! - rules and decisions are never archived on their own; the ones unused
//!   for three months are listed with a question: still valid?

use crate::index::{Index, update_note};
use crate::note::Kind;
use crate::output::{date, now, parse_date};
use crate::project::Project;
use crate::state::State;
use anyhow::Result;

/// Unused this long: archived (errors, notes) or questioned (rules, decisions).
const UNUSED: i64 = 90 * 86_400;
/// Errors seen this often stay, however old.
const KEEP_SEEN: i64 = 3;
/// Rules and decisions questioned at a time.
const QUESTIONED: usize = 5;

/// Runs the day's upkeep if it has not run today; what it did, for output.
pub fn daily(project: &Project, state: &State) -> Result<Vec<String>> {
    let today = date(now());
    if state.get("upkeep").as_deref() == Some(today.as_str()) {
        return Ok(Vec::new());
    }
    state.set("upkeep", &today)?;
    let index = Index::open(project)?;
    let cutoff = now() - UNUSED;
    let last_used = |id: &str, changed: i64, last_seen: Option<&str>| -> Result<i64> {
        let seen = last_seen.and_then(parse_date).unwrap_or(0);
        Ok(changed.max(seen).max(state.last_shown(id)?.unwrap_or(0)))
    };

    let mut archived = Vec::new();
    for kind in [Kind::Error, Kind::Note] {
        for n in index.notes_of(kind)? {
            let status = n.props["status"].as_str().unwrap_or("active");
            let seen = n.props["seen"].as_str().and_then(|s| s.parse::<i64>().ok()).unwrap_or(1);
            if status != "active" || (kind == Kind::Error && seen >= KEEP_SEEN) {
                continue;
            }
            if last_used(&n.id, n.changed, n.props["last_seen"].as_str())? < cutoff {
                update_note(project, &n.path, |note, _, _| {
                    note.props.set_text("status", "archived");
                    Ok(())
                })?;
                archived.push(n.id);
            }
        }
    }

    // only a project used for longer than that can have unused rules
    let mut questioned = Vec::new();
    if state.first_session()?.is_some_and(|t| t < cutoff) {
        for p in index.pieces_of(Kind::Rule)? {
            if state.last_shown(&p.id)?.unwrap_or(0) < cutoff {
                questioned.push(p.id);
            }
        }
        for n in index.notes_of(Kind::Decision)? {
            let active = n.props["status"].as_str().unwrap_or("active") == "active";
            if active && last_used(&n.id, n.changed, None)? < cutoff {
                questioned.push(n.id);
            }
        }
    }

    let mut out = super::secrets::in_notes_lines(project);
    if !archived.is_empty() {
        out.push(format!("archived (unused for 3 months; still found when searched for): {}", archived.join(", ")));
    }
    if !questioned.is_empty() {
        let more = questioned.len().saturating_sub(QUESTIONED);
        out.push(format!(
            "not used for 3 months, still valid? {}{}: pg rm <id> if not",
            questioned.iter().take(QUESTIONED).cloned().collect::<Vec<_>>().join(", "),
            if more > 0 { format!(" (+{more})") } else { String::new() }
        ));
    }
    Ok(out)
}
