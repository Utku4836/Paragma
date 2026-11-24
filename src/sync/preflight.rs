//! Before connecting, moving or backing up: what would go (files, size,
//! keys, files kept with notes) and whether the folder it goes to can take
//! it (written to, room enough). Lines for people, each well or not.

use super::repo::{kept, walk};
use crate::project::Home;
use std::path::Path;

/// One line of the summary, and whether it is well.
pub type Line = (String, bool);

/// What the memory sends: the projects that go, the .env files when they
/// go, the files kept with notes when they go (up to `limit_mb` each).
/// The lines, and the bytes that go.
pub fn memory(home: &Home, env: bool, files_on: bool, limit_mb: u64, skip: &[String]) -> (Vec<Line>, u64) {
    let projects = home.projects_dir();
    let mut all = Vec::new();
    let _ = walk(&projects, "", &mut all, &|rel, is_dir| kept(rel, is_dir, true));
    let attached: std::collections::HashMap<String, u64> = crate::project::attach::all(&projects).into_iter().collect();
    let size = |rel: &str| std::fs::metadata(projects.join(rel)).map(|m| m.len()).unwrap_or(0);
    let (mut files, mut bytes, mut keys, mut kept_n, mut kept_bytes, mut over) = (0, 0u64, 0, 0, 0u64, 0);
    let mut names = std::collections::BTreeSet::new();
    for rel in &all {
        let project = rel.split('/').next().unwrap_or("");
        if skip.iter().any(|s| s == project) {
            continue;
        }
        if rel.ends_with(".env") {
            keys += 1;
            if !env {
                continue;
            }
        }
        if let Some(s) = attached.get(rel) {
            kept_n += 1;
            kept_bytes += s;
            if !files_on {
                continue;
            }
            if *s > limit_mb * 1024 * 1024 {
                over += 1;
                continue;
            }
        }
        names.insert(project.to_string());
        files += 1;
        bytes += size(rel);
    }
    let mut out = vec![(
        format!(
            "{}, {}, in {}",
            crate::output::count(files as i64, "file"),
            mb(bytes),
            crate::output::count(names.len() as i64, "project")
        ),
        true,
    )];
    if keys > 0 {
        out.push((format!(".env files (keys): {keys}, {}", if env { "they go too" } else { "they stay here" }), true));
    }
    if kept_n > 0 {
        let how = if !files_on {
            "they stay here".to_string()
        } else if over > 0 {
            format!("they go, up to {limit_mb} MB each ({over} larger stay here)")
        } else {
            format!("they go, up to {limit_mb} MB each")
        };
        out.push((format!("files kept with notes: {kept_n}, {}: {how}", mb(kept_bytes)), true));
    }
    if !skip.is_empty() {
        out.push((format!("staying on this computer: {}", skip.join(", ")), true));
    }
    (out, bytes)
}

/// Whether `dir` (made when missing) can take `need` bytes: written to,
/// and room enough.
pub fn folder(dir: &Path, need: u64) -> Vec<Line> {
    let mut out = Vec::new();
    let probe = dir.join(format!(".paragma-probe-{}", std::process::id()));
    let made = !dir.exists();
    let writes = std::fs::create_dir_all(dir).is_ok() && std::fs::write(&probe, b"paragma").is_ok();
    let _ = std::fs::remove_file(&probe);
    if made {
        let _ = std::fs::remove_dir(dir);
    }
    out.push(if writes {
        (format!("{} can be written to", dir.display()), true)
    } else {
        (format!("{} cannot be written to", dir.display()), false)
    });
    if let Some(free) = free_space(dir) {
        let room = free > need.saturating_mul(2).max(need + 50 * 1024 * 1024);
        out.push((format!("{} free there{}", mb(free), if room { "" } else { ": not enough room" }), room));
    }
    out
}

fn mb(bytes: u64) -> String {
    let m = bytes as f64 / 1024.0 / 1024.0;
    if m >= 1024.0 {
        format!("{:.1} GB", m / 1024.0)
    } else if m >= 10.0 {
        format!("{m:.0} MB")
    } else {
        format!("{m:.1} MB")
    }
}

/// The room left on the disk of `p` (its nearest folder that is there).
#[cfg(windows)]
pub fn free_space(p: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let at = p.ancestors().find(|a| a.is_dir())?;
    let wide: Vec<u16> = at.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
pub fn free_space(p: &Path) -> Option<u64> {
    let at = p.ancestors().find(|a| a.is_dir())?;
    let out = std::process::Command::new("df").arg("-Pk").arg(at).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let kb: u64 = text.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()?;
    Some(kb * 1024)
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/sync/preflight.rs"]
mod tests;
