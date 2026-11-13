//! pg link: the folders a project's code is in. `pg open` links the folder
//! it runs in; this lists the links, adds one elsewhere, takes one out or
//! moves the project to another folder.
//!
//! ```text
//! pg link                         the project's folders (and which are gone)
//! pg link ../shop-theme           link a folder (default: this one)
//! pg link ../shop-theme --replace link it, and take the project's other folders out
//! pg link --remove ../old         take a link out (default: this folder); --gone: every folder that is gone
//! ```

use super::{Context, NOT_FOUND, USAGE, fail};
use crate::state::links::{Links, folder_key};
use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub struct Args {
    /// The folder (default: the one you are in)
    folder: Option<PathBuf>,
    /// Link it and take the project's other folders out (the code moved)
    #[arg(long, conflicts_with = "remove")]
    replace: bool,
    /// Take the link out instead
    #[arg(long)]
    remove: bool,
    /// With --remove: every linked folder that no longer exists
    #[arg(long, requires = "remove", conflicts_with = "folder")]
    gone: bool,
}

pub fn run(cx: &Context, a: Args) -> Result<()> {
    let Some(project) = cx.project.as_ref() else {
        return Err(fail(USAGE, "which project? pg link <folder> -p <project> (projects: pg ls)"));
    };
    let name = project.name.as_str();
    let folder = match &a.folder {
        Some(f) => std::path::absolute(if f.is_absolute() { f.clone() } else { cx.cwd.join(f) })?,
        None => cx.cwd.clone(),
    };
    let links = Links::load(&cx.home);
    let mine = links.folders_of(name);

    if a.remove {
        let only: Vec<String> = if a.gone {
            mine.iter().filter(|f| !Path::new(f.as_str()).is_dir()).cloned().collect()
        } else {
            let key = folder_key(&folder);
            if !mine.contains(&key) {
                return Err(fail(
                    NOT_FOUND,
                    format!("{} is not linked to {name} (pg link lists its folders)", show(&key)),
                ));
            }
            vec![key]
        };
        let out = Links::unlink(&cx.home, name, Some(&only))?;
        if out.is_empty() {
            println!("ok nothing to take out: every folder of {name} is there");
        }
        for f in out {
            println!("ok {} is no longer linked to {name}", show(&f));
        }
        return list(cx, name);
    }

    if a.folder.is_none() && !a.replace {
        return list(cx, name);
    }
    if !folder.is_dir() {
        return Err(fail(NOT_FOUND, format!("no folder {}", folder.display())));
    }
    if a.replace {
        let others: Vec<String> = mine.iter().filter(|f| **f != folder_key(&folder)).cloned().collect();
        for f in Links::unlink(&cx.home, name, Some(&others))? {
            println!("ok {} is no longer linked to {name}", show(&f));
        }
    }
    Links::link(&cx.home, &folder, name)?;
    println!("ok {} linked to {name}: commands there know the project, its code is in the map", folder.display());
    if folder.starts_with(&cx.home.root) {
        println!(
            "note: this folder is inside the memory ({}): its files go to GitHub with the notes, and .md files in it count as notes; code is better kept outside and linked",
            cx.home.root.display()
        );
    }
    list(cx, name)
}

/// The project's folders, each said to be there or gone.
fn list(cx: &Context, name: &str) -> Result<()> {
    let folders = Links::load(&cx.home).folders_of(name);
    if folders.is_empty() {
        println!("{name} has no folder linked: pg link <folder> (or pg open {name} in it)");
        return Ok(());
    }
    println!("folders of {name}:");
    let mut gone = 0;
    for f in &folders {
        let there = Path::new(f.as_str()).is_dir();
        gone += usize::from(!there);
        println!("  {}{}", show(f), if there { "" } else { "  (gone)" });
    }
    if gone > 0 {
        println!("take the gone ones out: pg link --remove --gone");
    }
    Ok(())
}

/// A folder key as people write it on this system.
fn show(key: &str) -> String {
    if cfg!(windows) { key.replace('/', "\\") } else { key.to_string() }
}
