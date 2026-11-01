//! pg skill: Paragma's instructions for agents. Without a name, the core
//! (what pg setup writes) and the list of skills; with a name, that skill.

use super::{Context, NOT_FOUND, fail};
use crate::skills;
use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
    /// coding, projects or knowledge
    name: Option<String>,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    skills::write_copies(&cx.home)?;
    let Some(name) = a.name else {
        print!("{}", skills::CORE);
        let dir = cx.home.root.join("skills").to_string_lossy().replace('\\', "/");
        println!("\nskills (copies in {dir}/paragma-<name>/SKILL.md):");
        for (name, text) in skills::SKILLS {
            let about = text.lines().find_map(|l| l.strip_prefix("description: ")).unwrap_or("");
            println!("  pg skill {name}  {about}");
        }
        return Ok(());
    };
    let Some((_, text)) = skills::SKILLS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())) else {
        let names: Vec<&str> = skills::SKILLS.iter().map(|(n, _)| *n).collect();
        return Err(fail(NOT_FOUND, format!("no skill {name:?} (skills: {})", names.join(", "))));
    };
    // without the front matter: that is for agents' skill folders
    let body = text.split_once("\n---\n").map(|(_, b)| b).unwrap_or(text);
    print!("{}", body.trim_start());
    Ok(())
}
