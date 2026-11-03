//! Whether a user's message gives the project a lasting rule ("from now on,
//! always run the tests with -race", "asla main'e push etme"), so the agent
//! can be asked to keep it. Only the agent writes it, and only if the user
//! meant it: this finds candidates, and should rather miss one than call
//! an ordinary request a rule.
//!
//! A sentence is a candidate when a word that makes things lasting (always,
//! never, from now on, in this project; her zaman, asla, bundan sonra,
//! artık) stands in an instruction: in English a verb comes right after it,
//! or "we" or a modal does the saying; in Turkish the verb is an order or a
//! convention (etme, kullan, olsun, olmalı, kullanmıyoruz), not a
//! description, the past or a question (döndürüyor, görmemiştim, mi).

/// The sentence of the message that gives a lasting rule, if one does.
pub fn find(message: &str) -> Option<String> {
    let text = without_quotes(message);
    sentences(&text).into_iter().find(|s| is_rule(s)).map(|s| s.trim().to_string())
}

/// Phrases that make an instruction lasting, wherever they stand.
const LASTING_EN: &[&str] = &[
    "from now on",
    "going forward",
    "in the future",
    "every time",
    "the rule here is",
    "the rule is",
    "remember:",
    "remember that",
    "please remember",
    "in this project",
    "in this repo",
    "in this codebase",
    "in this repository",
    "here,",
];
/// Words that make an instruction lasting when a verb follows them.
const ALWAYS_EN: &[&str] = &["always", "never"];
/// Verbs an instruction to a coding agent starts with.
const VERBS_EN: &[&str] = &[
    "add", "ask", "avoid", "build", "call", "change", "check", "commit", "create", "delete", "deploy", "document",
    "do", "edit", "format", "follow", "handle", "keep", "lint", "log", "make", "mark", "mask", "merge", "name", "open",
    "prefer", "push", "put", "read", "rebase", "remove", "rename", "return", "review", "run", "save", "sign", "sort",
    "squash", "store", "test", "touch", "update", "use", "validate", "wrap", "write",
];
/// Openings of a question.
const QUESTION_EN: &[&str] =
    &["why", "what", "how", "does", "do you", "is ", "are ", "can you", "could you", "would you", "will you"];

const LASTING_TR: &[&str] = &[
    "bundan sonra",
    "bundan böyle",
    "artık",
    "her zaman",
    "daima",
    "asla",
    "hiçbir zaman",
    "hiç bir zaman",
    "unutma",
    "bu projede",
    "bu repoda",
];
/// Endings of a Turkish verb that orders or states a convention.
const ORDER_TR: &[&str] = &[
    "ma", "me", // negative order: etme, kullanma
    "sın", "sin", "sun", "sün", // let it be: güncellensin, olsun
    "malı", "meli", // must: olmalı
    "mıyoruz", "miyoruz", "muyoruz", "müyoruz", // we do not: kullanmıyoruz
    "ıyoruz", "iyoruz", "uyoruz", "üyoruz", // we do: kullanıyoruz
    "ın", "in", "un", "ün", // polite order: yazın
];
/// Endings that describe, tell the past or can-not: not an order.
const NOT_ORDER_TR: &[&str] = &[
    "yor",
    "yorum",
    "yorsun",
    "dı",
    "di",
    "du",
    "dü",
    "tı",
    "ti",
    "tu",
    "tü",
    "mış",
    "miş",
    "muş",
    "müş",
    "mıştı",
    "mişti",
    "mıştım",
    "miştim",
    "emezdim",
    "amazdım",
    "ebilir",
    "abilir",
    "dır",
    "dir",
];
/// Bare orders common to coding work (the verb with no ending).
const STEMS_TR: &[&str] =
    &["yaz", "kullan", "çalıştır", "sor", "ekle", "sakla", "tut", "et", "yap", "güncelle", "sil", "koy", "bak", "test"];

fn is_rule(sentence: &str) -> bool {
    let lower = sentence.trim().to_lowercase();
    if lower.is_empty() || lower.ends_with('?') || lower.contains("never mind") {
        return false;
    }
    let words: Vec<&str> =
        lower.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '_').filter(|w| !w.is_empty()).collect();
    if turkish(&lower) { tr_rule(&lower, &words) } else { en_rule(&lower, &words) }
}

fn en_rule(lower: &str, words: &[&str]) -> bool {
    if QUESTION_EN.iter().any(|q| lower.starts_with(q)) {
        return false;
    }
    let verb = |w: Option<&&str>| w.is_some_and(|w| VERBS_EN.contains(w));
    // "always run", "never push", "we always deploy", "make sure you always format"
    let always = words.iter().enumerate().any(|(i, w)| {
        ALWAYS_EN.contains(w)
            && (verb(words.get(i + 1))
                || i.checked_sub(1).and_then(|j| words.get(j)).is_some_and(|p| matches!(*p, "we" | "you"))
                || lower.trim_end_matches(['.', '!']).ends_with(&format!(", {w}")))
    });
    // "don't ever", "do not ever"
    let ever = lower.contains("don't ever") || lower.contains("do not ever");
    // a lasting phrase, with an order (a clause that starts with a verb) or
    // a must in the sentence
    let lasting = LASTING_EN.iter().any(|p| lower.contains(p));
    let clause_starts = lower.split([',', ':']).any(|c| {
        let first = c.split(|ch: char| !ch.is_alphanumeric() && ch != '\'').find(|w| !w.is_empty());
        verb(first.as_ref())
    });
    let order = clause_starts
        || words.windows(2).any(|w| matches!(w[0], "don't" | "dont" | "not" | "stop" | "please") && verb(Some(&w[1])))
        || words.iter().any(|w| matches!(*w, "should" | "must" | "needs" | "need"));
    // "stop adding", "stop using"
    let stop = words.windows(2).any(|w| w[0] == "stop" && w[1].ends_with("ing") && w[1].len() > 4);
    // "every PR needs", "every migration must"
    let every = words.contains(&"every") && words.iter().any(|w| matches!(*w, "must" | "needs" | "should"));
    // "use X, not Y", "use X instead of Y", "don't use X, use Y"
    let instead = (words.first() == Some(&"use") && (lower.contains(", not ") || lower.contains("instead of")))
        || (lower.contains("don't use") && lower.contains(", use "));
    always || ever || stop || every || instead || (lasting && order)
}

fn tr_rule(lower: &str, words: &[&str]) -> bool {
    if words.iter().any(|w| matches!(*w, "mı" | "mi" | "mu" | "mü" | "neden" | "niye" | "nasıl" | "mısın" | "misin"))
    {
        return false;
    }
    if !LASTING_TR.iter().any(|p| lower.contains(p)) || lower.contains("olduğu gibi") {
        return false;
    }
    // the verb ends a clause in Turkish ("daima UTC kullan, yerel saat değil")
    let order = |w: &str| {
        let order = ORDER_TR.iter().any(|e| w.ends_with(e)) && w.chars().count() > 3;
        let not = NOT_ORDER_TR.iter().any(|e| w.ends_with(e));
        STEMS_TR.contains(&w) || (order && (!not || w.ends_with("yoruz")))
    };
    lower.split([',', ';', ':']).any(|clause| {
        clause
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .rfind(|w| !w.is_empty())
            .is_some_and(|w| order(w.rsplit('\'').next().unwrap_or(w)))
    })
}

/// Whether the text is Turkish: its letters, or its common words.
fn turkish(lower: &str) -> bool {
    lower.contains(['ç', 'ğ', 'ı', 'ö', 'ş', 'ü'])
        || lower.split_whitespace().any(|w| matches!(w, "bu" | "ve" | "bir" | "için" | "her" | "asla" | "artık"))
}

/// The message without what it quotes ("make it say \"never retry\"").
fn without_quotes(s: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' | '“' | '”' | '`' => quoted = !quoted,
            _ if !quoted => out.push(c),
            _ => {}
        }
    }
    out
}

/// The sentences of a message: split at . ! ? ; and line ends, keeping
/// "e.g." and file names together.
fn sentences(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        let next = chars.get(i + 1).copied();
        let end = match c {
            '\n' | '!' | '?' | ';' => true,
            '.' => next.is_none_or(char::is_whitespace),
            _ => false,
        };
        if end {
            out.push(std::mem::take(&mut cur));
        }
    }
    out.push(cur);
    out.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_and_lookalikes() {
        assert!(find("From now on, always run the tests with -race.").is_some());
        assert!(find("asla main'e direkt push etme").is_some());
        assert_eq!(find("Fix it. Never push directly to main.").as_deref(), Some("Never push directly to main."));
        assert!(find("The function always returns nil, that's the bug.").is_none());
        assert!(find("Never mind, let's do the other thing.").is_none());
        assert!(find("bu fonksiyon her zaman nil döndürüyor, sorun bu").is_none());
        assert!(find("Make the error message say \"never retry\".").is_none());
    }

    /// Precision and recall on bench/prompt-rules.tsv, on the dev split
    /// (SPLIT=held for the held-out one, measured once): cargo test
    /// --release said_rule -- --ignored --nocapture
    #[test]
    #[ignore]
    fn measure() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/bench/prompt-rules.tsv");
        let text = std::fs::read_to_string(path).unwrap();
        let split = std::env::var("SPLIT").unwrap_or_else(|_| "dev".into());
        {
            let split = split.as_str();
            let (mut tp, mut fp, mut fn_) = (0, 0, 0);
            for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
                let parts: Vec<&str> = line.splitn(3, '\t').collect();
                if parts[0] != split {
                    continue;
                }
                let (rule, found) = (parts[1] == "rule", find(parts[2]).is_some());
                match (rule, found) {
                    (true, true) => tp += 1,
                    (false, true) => {
                        fp += 1;
                        println!("{split} false alarm: {}", parts[2]);
                    }
                    (true, false) => {
                        fn_ += 1;
                        println!("{split} missed: {}", parts[2]);
                    }
                    _ => {}
                }
            }
            println!("{split}: found {tp} of {} rules, {fp} false alarms", tp + fn_);
        }
    }
}
