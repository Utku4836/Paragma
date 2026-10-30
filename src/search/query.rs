//! Understanding a query: what kind it is and which words to search for.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryKind {
    /// An error message: exact words matter most.
    ErrorLine,
    /// A file path or a code name.
    Code,
    /// A question or a sentence: meaning matters most.
    Sentence,
    /// A few topic words.
    Words,
}

pub fn classify(q: &str) -> QueryKind {
    let lower = q.to_lowercase();
    let error_marks = [
        "error",
        "panic",
        "fail",
        "exception",
        "traceback",
        "mismatch",
        "undefined",
        "cannot",
        "expected",
        "locked",
        "denied",
        "not found",
        "timeout",
        "timed out",
        "refused",
    ];
    let has_digits = q.chars().any(|c| c.is_ascii_digit());
    let marked = error_marks.iter().any(|m| lower.contains(m)) && q.split_whitespace().count() >= 3;
    if (q.contains(':') && has_digits) || marked {
        return QueryKind::ErrorLine;
    }
    let words = q.split_whitespace().count();
    let codey = q.contains('/') || q.contains("::") || q.contains('#') || q.contains('_') || has_camel_case(q);
    if words <= 2 && codey {
        return QueryKind::Code;
    }
    if words >= 4 { QueryKind::Sentence } else { QueryKind::Words }
}

fn has_camel_case(s: &str) -> bool {
    s.split_whitespace().any(|w| {
        let mut chars = w.chars();
        chars.next().is_some() && w.chars().skip(1).any(|c| c.is_uppercase()) && w.chars().any(|c| c.is_lowercase())
    })
}

/// Words that carry no topic, in English and Turkish.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "of", "to", "in", "on", "at", "for", "with", "by", "from", "is", "are", "was",
    "were", "be", "been", "do", "does", "did", "how", "what", "which", "when", "where", "why", "who", "can", "could",
    "should", "would", "will", "we", "you", "it", "its", "this", "that", "these", "those", "our", "my", "your",
    "their", "there", "get", "has", "have", "had", "not", "no", "as", "if", "then", "than", "so", "about", "into",
    "after", "before", "any", "some", "all", "ve", "ile", "bir", "bu", "şu", "da", "de", "mi", "mı", "mu", "mü", "ne",
    "nasıl", "neden", "hangi", "hangisi", "için", "gibi", "daha", "çok", "en", "kaç", "nerede", "ya", "veya", "ama",
    "sonra", "önce", "içinde", "olan", "olarak",
];

/// The words to search for: the query's words that carry a topic, lower
/// case, code names also split into their parts (ErrRefundWindow -> err
/// refund window).
pub fn words(q: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |w: &str| {
        let w = w.to_lowercase();
        if w.chars().count() >= 2 && !STOP_WORDS.contains(&w.as_str()) && !out.contains(&w) {
            out.push(w);
        }
    };
    for token in q.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if token.is_empty() {
            continue;
        }
        add(token);
        let parts = split_code_name(token);
        if parts.len() > 1 {
            for p in &parts {
                add(p);
            }
        }
    }
    out
}

/// "ErrRefundWindow" -> [Err, Refund, Window]; "user_id" -> [user, id].
fn split_code_name(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in s.split('_') {
        let mut cur = String::new();
        let chars: Vec<char> = part.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            let prev_lower = i > 0 && chars[i - 1].is_lowercase();
            if c.is_uppercase() && !cur.is_empty() && (prev_lower || next_lower) {
                out.push(std::mem::take(&mut cur));
            }
            cur.push(c);
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(classify("catalog_test.go:8: catalog checksum mismatch: got 8c77 want 4ff5"), QueryKind::ErrorLine);
        assert_eq!(classify("database is locked (5)"), QueryKind::ErrorLine);
        assert_eq!(classify("payments/refund.go"), QueryKind::Code);
        assert_eq!(classify("ErrRefundWindow"), QueryKind::Code);
        assert_eq!(classify("how long can a customer get a refund"), QueryKind::Sentence);
        assert_eq!(classify("refund"), QueryKind::Words);
    }

    #[test]
    fn code_names_are_split() {
        assert_eq!(words("ErrRefundWindow"), vec!["errrefundwindow", "err", "refund", "window"]);
        assert_eq!(words("user_id HTTPServer"), vec!["user_id", "user", "id", "httpserver", "http", "server"]);
        assert_eq!(words("para iadesi"), vec!["para", "iadesi"]);
        assert_eq!(words("how do I change the database schema"), vec!["change", "database", "schema"]);
    }
}
