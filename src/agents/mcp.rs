//! Paragma as an MCP server in the apps that start MCP servers: where each
//! keeps its servers, putting `pg mcp` there and taking it out. Always for
//! every conversation (the apps keep MCP servers globally). Every other
//! server and setting in those files stays as it is.
//!
//! | App            | Where                                              |
//! |----------------|----------------------------------------------------|
//! | Claude Code    | `claude mcp add --scope user`, else `~/.claude.json` |
//! | Claude Desktop | `claude_desktop_config.json` in its app folder     |
//! | Codex          | `codex mcp add`, else `~/.codex/config.toml`       |
//! | Antigravity    | `~/.gemini/config/mcp_config.json`                 |
//! | Cursor         | `~/.cursor/mcp.json`                               |

use super::hooks::Change;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The name Paragma's server has in every app.
pub const NAME: &str = "paragma";

/// An app that starts MCP servers.
pub struct Client {
    pub id: &'static str,
    pub name: &'static str,
    /// The files it keeps its servers in (the first is the one it reads;
    /// others are older places it may still read).
    pub files: Vec<PathBuf>,
    /// Its own command for adding servers, when found ("claude", "codex").
    pub cli: Option<PathBuf>,
    pub installed: bool,
}

fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_default()
}

fn codex_home() -> PathBuf {
    match std::env::var_os("CODEX_HOME") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => home().join(".codex"),
    }
}

/// Claude Desktop's folder: %APPDATA%\Claude, ~/Library/Application
/// Support/Claude, ~/.config/Claude.
fn desktop_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default().join("Claude")
    } else if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support").join("Claude")
    } else {
        home().join(".config").join("Claude")
    }
}

/// Tests set PARAGMA_MCP_FILES to write the files even when the app's own
/// command is there.
fn use_cli() -> bool {
    std::env::var_os("PARAGMA_MCP_FILES").is_none()
}

/// Every app pg can add its server to, found here or not.
pub fn clients() -> Vec<Client> {
    let h = home();
    let claude_cli = super::which("claude").filter(|_| use_cli());
    let codex_cli = super::which("codex").filter(|_| use_cli());
    let gemini = h.join(".gemini");
    let mut antigravity = vec![gemini.join("config").join("mcp_config.json")];
    // an older place, still read by some versions
    let older = gemini.join("antigravity").join("mcp_config.json");
    if older.is_file() {
        antigravity.push(older);
    }
    vec![
        Client {
            id: "claude",
            name: "Claude Code",
            files: vec![h.join(".claude.json")],
            installed: super::which("claude").is_some() || h.join(".claude").is_dir(),
            cli: claude_cli,
        },
        Client {
            id: "claude-desktop",
            name: "Claude Desktop",
            files: vec![desktop_dir().join("claude_desktop_config.json")],
            installed: desktop_dir().is_dir(),
            cli: None,
        },
        Client {
            id: "codex",
            name: "Codex",
            files: vec![codex_home().join("config.toml")],
            installed: super::which("codex").is_some() || codex_home().is_dir(),
            cli: codex_cli,
        },
        Client {
            id: "antigravity",
            name: "Antigravity",
            installed: gemini.join("antigravity").is_dir()
                || gemini.join("config").is_dir()
                || gemini.join("antigravity-cli").is_dir()
                || super::which("agy").is_some(),
            files: antigravity,
            cli: None,
        },
        Client {
            id: "cursor",
            name: "Cursor",
            files: vec![h.join(".cursor").join("mcp.json")],
            installed: h.join(".cursor").is_dir(),
            cli: None,
        },
    ]
}

pub fn find(id: &str) -> Option<Client> {
    clients().into_iter().find(|c| c.id.eq_ignore_ascii_case(id) || c.name.eq_ignore_ascii_case(id))
}

/// The arguments the app starts pg with: it says which app it is, so pg
/// knows whether hooks already give that app the brief.
fn args(c: &Client) -> Vec<String> {
    vec!["mcp".into(), "--client".into(), c.id.into()]
}

/// Whether the app has Paragma's server.
pub fn installed(c: &Client) -> bool {
    let Some(file) = c.files.first() else { return false };
    if c.id == "codex" {
        return std::fs::read_to_string(file).is_ok_and(|t| toml_block(&t).is_some());
    }
    read_json(file).is_ok_and(|v| v["mcpServers"][NAME].is_object())
}

/// Puts (or updates) Paragma's server in the app.
pub fn install(c: &Client, exe: &Path) -> Result<Change> {
    if let Some(cli) = &c.cli {
        // the app's own command: remove the old entry, add the new one
        let _ = run(cli, &remove_args(c));
        let mut a: Vec<String> = match c.id {
            "claude" => vec!["mcp".into(), "add".into(), "--scope".into(), "user".into(), NAME.into(), "--".into()],
            _ => vec!["mcp".into(), "add".into(), NAME.into(), "--".into()],
        };
        a.push(exe.to_string_lossy().into_owned());
        a.extend(args(c));
        run(cli, &a)?;
        return Ok(Change::Written);
    }
    let mut changed = Change::Unchanged;
    for file in &c.files {
        let done = if c.id == "codex" { toml_install(file, exe, &args(c))? } else { json_install(c, file, exe)? };
        if done == Change::Written {
            changed = Change::Written;
        }
    }
    Ok(changed)
}

/// Takes Paragma's server out of the app.
pub fn remove(c: &Client) -> Result<Change> {
    if let Some(cli) = &c.cli {
        if !installed(c) {
            return Ok(Change::Unchanged);
        }
        run(cli, &remove_args(c))?;
        return Ok(Change::Written);
    }
    let mut changed = Change::Unchanged;
    for file in c.files.iter().filter(|f| f.is_file()) {
        let done = if c.id == "codex" { toml_remove(file)? } else { json_remove(file)? };
        if done == Change::Written {
            changed = Change::Written;
        }
    }
    Ok(changed)
}

fn remove_args(c: &Client) -> Vec<String> {
    match c.id {
        "claude" => vec!["mcp".into(), "remove".into(), "--scope".into(), "user".into(), NAME.into()],
        _ => vec!["mcp".into(), "remove".into(), NAME.into()],
    }
}

fn run(cli: &Path, args: &[String]) -> Result<()> {
    let o = std::process::Command::new(cli)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("run {}", cli.display()))?;
    if !o.status.success() {
        bail!(
            "{} {}: {}",
            cli.file_name().unwrap_or_default().to_string_lossy(),
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        );
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        Ok(text) => {
            let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
                .with_context(|| format!("{} is not valid JSON; it was left as it is", path.display()))?;
            if !v.is_object() {
                bail!("{} is not a JSON object; it was left as it is", path.display());
            }
            Ok(v)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

fn write_json(path: &Path, before: &Value, after: &Value) -> Result<Change> {
    if before == after {
        return Ok(Change::Unchanged);
    }
    crate::project::files::write_atomic(path, (serde_json::to_string_pretty(after)? + "\n").as_bytes())?;
    Ok(Change::Written)
}

fn json_install(c: &Client, path: &Path, exe: &Path) -> Result<Change> {
    let before = read_json(path)?;
    let mut doc = before.clone();
    let root = doc.as_object_mut().expect("read gives an object");
    let servers = root.entry("mcpServers").or_insert_with(|| json!({}));
    let Some(servers) = servers.as_object_mut() else {
        bail!("{}: \"mcpServers\" is not an object; it was left as it is", path.display());
    };
    let mut entry = json!({ "command": exe.to_string_lossy(), "args": args(c) });
    if c.id == "claude" {
        entry["type"] = json!("stdio");
        entry["env"] = json!({});
    }
    servers.insert(NAME.into(), entry);
    write_json(path, &before, &doc)
}

fn json_remove(path: &Path) -> Result<Change> {
    let before = read_json(path)?;
    let mut doc = before.clone();
    if let Some(s) = doc["mcpServers"].as_object_mut() {
        s.remove(NAME);
    }
    write_json(path, &before, &doc)
}

/// Where Paragma's table is in a config.toml: its first line and the line
/// after its last (its subtables, `[mcp_servers.paragma.env]`, with it).
fn toml_block(text: &str) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let head = format!("[mcp_servers.{NAME}]");
    let sub = format!("[mcp_servers.{NAME}.");
    let start = lines.iter().position(|l| l.trim() == head)?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, l)| l.trim_start().starts_with('[') && !l.trim_start().starts_with(&sub))
        .map(|(i, _)| i)
        .unwrap_or(lines.len());
    Some((start, end))
}

fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "/").replace('"', "\\\""))
}

fn toml_install(path: &Path, exe: &Path, args: &[String]) -> Result<Change> {
    let before = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    let table = format!(
        "[mcp_servers.{NAME}]\ncommand = {}\nargs = [{}]\n",
        toml_string(&exe.to_string_lossy()),
        args.iter().map(|a| toml_string(a)).collect::<Vec<_>>().join(", ")
    );
    let lines: Vec<&str> = before.lines().collect();
    let mut out: Vec<String> = Vec::new();
    match toml_block(&before) {
        Some((a, b)) => {
            out.extend(lines[..a].iter().map(|l| l.to_string()));
            out.extend(table.lines().map(str::to_string));
            if b < lines.len() {
                out.push(String::new());
                out.extend(lines[b..].iter().map(|l| l.to_string()));
            }
        }
        None => {
            out.extend(lines.iter().map(|l| l.to_string()));
            while out.last().is_some_and(|l| l.trim().is_empty()) {
                out.pop();
            }
            if !out.is_empty() {
                out.push(String::new());
            }
            out.extend(table.lines().map(str::to_string));
        }
    }
    let after = out.join("\n") + "\n";
    if after == before {
        return Ok(Change::Unchanged);
    }
    crate::project::files::write_atomic(path, after.as_bytes())?;
    Ok(Change::Written)
}

fn toml_remove(path: &Path) -> Result<Change> {
    let before = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let Some((a, b)) = toml_block(&before) else { return Ok(Change::Unchanged) };
    let lines: Vec<&str> = before.lines().collect();
    let mut out: Vec<&str> = lines[..a].to_vec();
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    if b < lines.len() {
        if !out.is_empty() {
            out.push("");
        }
        out.extend(&lines[b..]);
    }
    let after = if out.is_empty() { String::new() } else { out.join("\n") + "\n" };
    crate::project::files::write_atomic(path, after.as_bytes())?;
    Ok(Change::Written)
}

/// Whether Paragma's hooks already give this app's conversations the
/// brief (then the server does not say it again).
pub fn hooks_give_the_brief(client: &str, cwd: &Path) -> bool {
    if !super::hooks::AGENTS.contains(&client) {
        return false;
    }
    super::hooks::installed(&super::hooks::file(client, None))
        || cwd.ancestors().any(|d| super::hooks::installed(&super::hooks::file(client, Some(d))))
}
