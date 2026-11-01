//! The project's env file (`<project>/<project>.env`): its secrets, kept
//! out of the notes. Every project starts with an empty one.
//!
//! ```text
//! # Npm Api
//! NPM_API=npm_AbC123...
//! ```
//!
//! A secret is written with a title (`pg write --secret "Npm Api" <value>`)
//! and only its title, its variable and the first characters of its value
//! are ever shown. `pg run` hands the variables to the commands it runs, so
//! an agent uses a key without seeing it.

use super::{Project, files};
use anyhow::Result;
use std::path::PathBuf;

const HEADER: &str = "# Secrets of this project, kept out of the notes. Paragma shows only their titles.\n\
# Add one: pg write --secret \"<title>\" <value>. Use it: pg run -- <command using $NAME>.\n";

#[derive(Clone, Debug, PartialEq)]
pub struct Secret {
    pub title: String,
    /// The variable: NPM_API.
    pub name: String,
    pub value: String,
}

impl Secret {
    /// "Npm Api ($NPM_API, npm_***)": what output may show.
    pub fn show(&self) -> String {
        format!("{} (${}, {})", self.title, self.name, preview(&self.value))
    }
}

pub fn path(project: &Project) -> PathBuf {
    project.dir.join(format!("{}.env", project.name))
}

/// Creates the empty env file of a new project.
pub fn create(project: &Project) -> Result<()> {
    let p = path(project);
    if !p.exists() {
        files::write_atomic(&p, HEADER.as_bytes())?;
        private(&p);
    }
    Ok(())
}

pub fn load(project: &Project) -> Vec<Secret> {
    let Ok(text) = files::read_text(&path(project)) else { return Vec::new() };
    let mut out = Vec::new();
    let mut title = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(t) = line.strip_prefix('#') {
            title = t.trim().to_string();
        } else if let Some((name, value)) = line.split_once('=') {
            let name = name.trim().trim_start_matches("export ").trim().to_string();
            let title = if title.is_empty() { name.clone() } else { std::mem::take(&mut title) };
            out.push(Secret { title, name, value: unquote(value.trim()) });
        }
    }
    out
}

/// Adds a secret, or replaces the one with the same title. Returns it and
/// whether it was new.
pub fn set(project: &Project, title: &str, value: &str) -> Result<(Secret, bool)> {
    let _lock = files::WriteLock::take(&project.meta_dir())?;
    let mut all = load(project);
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let (secret, new) = match all.iter_mut().find(|s| s.title.eq_ignore_ascii_case(&title)) {
        Some(s) => {
            s.value = value.to_string();
            (s.clone(), false)
        }
        None => {
            let base = var_name(&title);
            let mut name = base.clone();
            let mut n = 2;
            while all.iter().any(|s| s.name == name) {
                name = format!("{base}_{n}");
                n += 1;
            }
            let s = Secret { title, name, value: value.to_string() };
            all.push(s.clone());
            (s, true)
        }
    };
    let mut text = HEADER.to_string();
    for s in &all {
        text.push_str(&format!("\n# {}\n{}={}\n", s.title, s.name, quote(&s.value)));
    }
    let p = path(project);
    files::write_atomic(&p, text.as_bytes())?;
    private(&p);
    Ok((secret, new))
}

/// "secrets: 3 (Npm Api, Stripe Live, Stripe Test): use $NAME in pg run",
/// or None for a project without secrets.
pub fn summary(project: &Project) -> Option<String> {
    let all = load(project);
    if all.is_empty() {
        return None;
    }
    let titles: Vec<&str> = all.iter().take(8).map(|s| s.title.as_str()).collect();
    let more = all.len().saturating_sub(8);
    Some(format!(
        "secrets: {} ({}{}): pg find <which> gives the $NAME to use in pg run",
        all.len(),
        titles.join(", "),
        if more > 0 { format!(", +{more}") } else { String::new() }
    ))
}

/// Words that say "a secret" rather than which one.
const GENERIC: &[&str] = &[
    "api",
    "key",
    "keys",
    "token",
    "tokens",
    "secret",
    "secrets",
    "password",
    "pass",
    "credential",
    "credentials",
    "access",
    "auth",
    "env",
    "the",
    "a",
    "for",
    "of",
    "anahtar",
    "anahtari",
    "anahtarı",
    "sifre",
    "şifre",
];

fn words(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// The secrets a query asks for: those most of whose title words it uses
/// ("stripe live key" -> Stripe Live). A query of only generic words
/// ("api keys") lists them all.
pub fn matching<'a>(secrets: &'a [Secret], query: &str) -> Vec<&'a Secret> {
    let q = words(query);
    if q.is_empty() {
        return Vec::new();
    }
    if q.iter().all(|w| GENERIC.contains(&w.as_str())) {
        return secrets.iter().collect();
    }
    let mut scored: Vec<(&Secret, f32)> = secrets
        .iter()
        .filter_map(|s| {
            let all = words(&s.title);
            let specific: Vec<&String> = all.iter().filter(|w| !GENERIC.contains(&w.as_str())).collect();
            let title: Vec<&String> = if specific.is_empty() { all.iter().collect() } else { specific };
            let hit = title.iter().filter(|w| q.contains(w)).count();
            let score = hit as f32 / title.len().max(1) as f32;
            (hit > 0 && score >= 0.5).then_some((s, score))
        })
        .collect();
    let best = scored.iter().map(|x| x.1).fold(0.0, f32::max);
    scored.retain(|x| x.1 >= best);
    scored.into_iter().map(|x| x.0).collect()
}

/// A command line with $NAME and ${NAME} of the secrets put in, the same
/// on every system (cmd would want %NAME%).
pub fn substitute(command: &str, secrets: &[Secret]) -> String {
    let mut sorted: Vec<&Secret> = secrets.iter().collect();
    sorted.sort_by_key(|s| std::cmp::Reverse(s.name.len())); // NPM_API_2 before NPM_API
    let mut out = command.to_string();
    for s in sorted {
        out = out.replace(&format!("${{{}}}", s.name), &s.value);
        let mut rest = out.as_str();
        let mut done = String::new();
        let mark = format!("${}", s.name);
        while let Some(i) = rest.find(&mark) {
            let after = &rest[i + mark.len()..];
            let whole = !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
            done.push_str(&rest[..i]);
            done.push_str(if whole { &s.value } else { &mark });
            rest = after;
        }
        done.push_str(rest);
        out = done;
    }
    out
}

/// Output with every secret's value replaced by its $NAME.
pub fn hide(text: &str, secrets: &[Secret]) -> String {
    let mut sorted: Vec<&Secret> = secrets.iter().filter(|s| s.value.chars().count() >= 4).collect();
    sorted.sort_by_key(|s| std::cmp::Reverse(s.value.len()));
    let mut out = text.to_string();
    for s in sorted {
        out = out.replace(&s.value, &format!("${}", s.name));
    }
    out
}

/// "Npm Api" -> NPM_API.
pub fn var_name(title: &str) -> String {
    let words: Vec<String> = title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_uppercase())
        .collect();
    let name = words.join("_");
    match name.chars().next() {
        None => "SECRET".to_string(),
        Some(c) if c.is_ascii_digit() => format!("S_{name}"),
        _ => name,
    }
}

/// The first characters of a value, enough to tell keys apart: "npm_***".
pub fn preview(value: &str) -> String {
    let n = value.chars().count();
    if n < 12 { "***".to_string() } else { format!("{}***", value.chars().take(4).collect::<String>()) }
}

fn quote(v: &str) -> String {
    if v.chars().all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\'' | '#' | '\\')) {
        v.to_string()
    } else {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
    }
}

fn unquote(v: &str) -> String {
    if let Some(inner) = v.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        return inner.replace("\\n", "\n").replace("\\\"", "\"").replace("\\\\", "\\");
    }
    if let Some(inner) = v.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        return inner.to_string();
    }
    v.to_string()
}

/// Only its owner may read it (where the system has such permissions).
fn private(p: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = p;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_kept_by_title() {
        let d = tempfile::tempdir().unwrap();
        let p = Project { name: "shop".into(), dir: d.path().to_path_buf() };
        create(&p).unwrap();
        assert!(load(&p).is_empty());
        let (s, new) = set(&p, "Npm  Api", "npm_AbCdEf123456").unwrap();
        assert!(new);
        assert_eq!((s.name.as_str(), s.show().as_str()), ("NPM_API", "Npm Api ($NPM_API, npm_***)"));
        set(&p, "Stripe Live", "sk_live \"quoted\" #x").unwrap();
        let (s, new) = set(&p, "npm api", "npm_New9876543210").unwrap();
        assert!(!new);
        assert_eq!(s.name, "NPM_API");
        let all = load(&p);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].value, "npm_New9876543210");
        assert_eq!(all[1].value, "sk_live \"quoted\" #x");
        assert_eq!(var_name("2fa seed"), "S_2FA_SEED");
    }

    #[test]
    fn titles_values_and_commands() {
        let s = |t: &str, n: &str, v: &str| Secret { title: t.into(), name: n.into(), value: v.into() };
        let all = vec![
            s("Stripe Live", "STRIPE_LIVE", "sk_live_AAAA1111"),
            s("Stripe Test", "STRIPE_TEST", "sk_test_BBBB2222"),
            s("Npm Api", "NPM_API", "npm_CCCC3333"),
            s("Npm Api 2", "NPM_API_2", "npm_DDDD4444"),
        ];
        let titles = |q: &str| matching(&all, q).iter().map(|s| s.title.clone()).collect::<Vec<_>>();
        assert_eq!(titles("stripe live key"), vec!["Stripe Live"]);
        assert_eq!(titles("stripe key"), vec!["Stripe Live", "Stripe Test"]);
        assert_eq!(titles("npm publish token"), vec!["Npm Api"]);
        assert_eq!(titles("api keys").len(), 4);
        assert!(titles("shader compile").is_empty());
        assert_eq!(
            substitute("curl -u $STRIPE_TEST: x ${NPM_API} $NPM_API_2 $NPM_APIX", &all),
            "curl -u sk_test_BBBB2222: x npm_CCCC3333 npm_DDDD4444 $NPM_APIX"
        );
        assert_eq!(hide("got sk_live_AAAA1111 back", &all), "got $STRIPE_LIVE back");
    }
}
