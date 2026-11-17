//! pg tell: a short note for the other conversations working on the
//! project ("I am changing refund.go, leave it"). Each one sees it once, at
//! its next brief or pg command, for a day.

use super::{Context, USAGE, fail};
use crate::state::State;
use crate::state::journal::Did;
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// What the others should know
    #[arg(required = true, allow_hyphen_values = true)]
    text: Vec<String>,
}

pub fn run(cx: &Context, mut a: Args) -> Result<()> {
    let project = cx.need_project()?;
    let warning = super::mask_all(a.text.iter_mut());
    let text = a.text.join(" ").trim().to_string();
    if text.is_empty() {
        return Err(fail(USAGE, "say what: pg tell \"...\""));
    }
    let state = State::open(project)?;
    state.record(Did::Told { text })?;
    state.touch_session(&cx.cwd.to_string_lossy())?;
    println!("ok told: the other conversations here see it once, at their next pg command (for a day)");
    if let Some(w) = warning {
        println!("{w}");
    }
    Ok(())
}
