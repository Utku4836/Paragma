//! Keeping secrets out of memory. Whatever is written to a note comes back
//! in every search that finds it, so API keys, tokens, private keys and
//! passwords an agent pastes are masked before the note is written, and
//! the write says so.

use regex::Regex;
use std::sync::OnceLock;

/// What replaces a secret.
pub const MASK: &str = "[secret removed]";

fn patterns() -> &'static [(&'static str, Regex)] {
    static P: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    P.get_or_init(|| {
        [
            ("a private key", r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?(-----END [A-Z ]*PRIVATE KEY-----|$)"),
            ("an AWS access key", r"\b(AKIA|ASIA)[0-9A-Z]{16}\b"),
            ("a GitHub token", r"\b(gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})\b"),
            ("an Anthropic key", r"\bsk-ant-[A-Za-z0-9_\-]{20,}"),
            ("an API key", r"\bsk-(proj-)?[A-Za-z0-9_\-]{20,}"),
            ("a Stripe key", r"\b(sk|rk)_live_[A-Za-z0-9]{16,}\b"),
            ("a Slack token", r"\bxox[abprs]-[A-Za-z0-9\-]{10,}"),
            ("a Google API key", r"\bAIza[0-9A-Za-z_\-]{35}\b"),
            ("an npm token", r"\bnpm_[A-Za-z0-9]{36}\b"),
            ("a JSON web token", r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}"),
            ("a password in a connection string", r"(?i)\b[a-z][a-z0-9+.\-]*://[^\s:/@]+:([^\s@/]{4,})@"),
        ]
        .into_iter()
        .map(|(what, re)| (what, Regex::new(re).expect("secret pattern")))
        .collect()
    })
}

/// `password: hunter22`, `API_KEY="abc..."`, `token = ...`: the value is
/// masked unless it is a placeholder (<...>, ${...}, ***, "see vault").
fn assignment() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r#"(?i)\b(password|passwd|pwd|secret|client_secret|api[_-]?key|access[_-]?key|auth[_-]?token|token|private[_-]?key)\b(["']?\s*[:=]\s*["']?)([^\s"',;]{8,})"#,
        )
        .expect("assignment pattern")
    })
}

/// The text with its secrets masked, and what kinds were found.
pub fn mask(text: &str) -> (String, Vec<&'static str>) {
    let mut out = text.to_string();
    let mut found = Vec::new();
    for (what, re) in patterns() {
        if re.is_match(&out) {
            found.push(*what);
            out = if *what == "a password in a connection string" {
                re.replace_all(&out, |c: &regex::Captures| c[0].replace(&c[1], MASK)).into_owned()
            } else {
                re.replace_all(&out, MASK).into_owned()
            };
        }
    }
    let mut assigned = false;
    out = assignment()
        .replace_all(&out, |c: &regex::Captures| {
            let value = &c[3];
            let placeholder = value.starts_with('<')
                || value.starts_with("${")
                || value.starts_with('$')
                || value.contains(MASK)
                || value.chars().all(|ch| ch == '*' || ch == 'x' || ch == 'X')
                || value.contains('[');
            if placeholder {
                c[0].to_string()
            } else {
                assigned = true;
                format!("{}{}{MASK}", &c[1], &c[2])
            }
        })
        .into_owned();
    if assigned {
        found.push("a password or key");
    }
    (out, found)
}

/// "masked 2 secrets (an AWS access key, a GitHub token): keep secrets in
/// the environment or a vault, notes only say where they are"
pub fn warning(found: &[&str]) -> Option<String> {
    if found.is_empty() {
        return None;
    }
    let mut kinds: Vec<&str> = found.to_vec();
    kinds.dedup();
    Some(format!(
        "masked {} ({}), not kept: to keep one, pg write --secret \"<title>\" <value>, then use it as $NAME in pg run",
        crate::output::count(found.len() as i64, "secret"),
        kinds.join(", ")
    ))
}

/// Notes with a key in them (written into the file outside `pg write`,
/// which masks them): their paths and what was found. The memory may go
/// to GitHub with them.
pub fn in_notes(project: &crate::project::Project) -> Vec<(String, Vec<&'static str>)> {
    let mut out = Vec::new();
    for f in crate::project::scan::notes(&project.dir) {
        let Ok(text) = std::fs::read_to_string(project.dir.join(&f.rel)) else { continue };
        let (_, mut found) = mask(&text);
        if !found.is_empty() {
            found.dedup();
            out.push((f.rel, found));
        }
    }
    out
}

/// The lines that say it, for the brief's daily check and pg doctor.
pub fn in_notes_lines(project: &crate::project::Project) -> Vec<String> {
    in_notes(project)
        .into_iter()
        .take(5)
        .map(|(rel, found)| {
            format!(
                "a key in a note: {rel} ({}): keep it with pg write --secret \"<title>\" <value>, then take it out of the note",
                found.join(", ")
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_masked() {
        let (out, found) = mask("deploy with AKIAIOSFODNN7EXAMPLE and ghp_abcdefghijklmnopqrstuvwxyz0123456789");
        assert_eq!(out, format!("deploy with {MASK} and {MASK}"));
        assert_eq!(found, vec!["an AWS access key", "a GitHub token"]);
        let (out, _) = mask("db: postgres://shop:s3cretpass@localhost/shop");
        assert_eq!(out, format!("db: postgres://shop:{MASK}@localhost/shop"));
        let (out, _) = mask("API_KEY=\"abcd1234efgh5678\" and password: hunter2222");
        assert_eq!(out, format!("API_KEY=\"{MASK}\" and password: {MASK}"));
        let (out, _) = mask("-----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY-----\nafter");
        assert_eq!(out, format!("{MASK}\nafter"));
    }

    #[test]
    fn placeholders_and_plain_words_stay() {
        for text in [
            "password: <from the vault>",
            "API_KEY=${STRIPE_KEY}",
            "token = ********",
            "The token is read from the environment.",
            "Use sk-learn for the model.",
            "password reset emails expire after 30 minutes",
        ] {
            assert_eq!(mask(text).0, text, "{text}");
        }
    }
}
