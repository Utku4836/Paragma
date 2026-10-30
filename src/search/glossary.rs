//! Software and business terms across Latin-script languages
//! (glossary.txt): a query word found there is also searched as the other
//! words of its concept, so "kdv" finds notes about "vat" and "iadesi" notes
//! about "refund". The meaning model alone does not link such words.

use std::sync::OnceLock;

static TEXT: &str = include_str!("glossary.txt");

/// The concepts, each a list of lower-case words and phrases.
fn concepts() -> &'static [Vec<String>] {
    static C: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    C.get_or_init(|| {
        TEXT.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.split('|').map(|w| w.trim().to_lowercase()).filter(|w| !w.is_empty()).collect())
            .collect()
    })
}

/// Words of the concepts the query mentions, besides the ones it uses; only
/// single words (what word search can look for).
pub fn related_words(query: &str, words: &[String]) -> Vec<String> {
    let lower = query.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for concept in concepts() {
        let mentioned = concept.iter().any(|term| {
            if term.contains(' ') { lower.contains(term.as_str()) } else { words.iter().any(|w| matches(w, term)) }
        });
        if !mentioned {
            continue;
        }
        for term in concept.iter().filter(|t| !t.contains(' ')) {
            if !words.contains(term) && !out.contains(term) {
                out.push(term.clone());
            }
        }
    }
    out
}

/// A query word matches a glossary word when they are equal, or one starts
/// with the other and the rest is a short ending (iade -> iadesi, redondear
/// -> redondea). Words of three letters or fewer only match whole, and
/// four-letter ones take endings of two letters at most ("para" is not
/// "paralel").
fn matches(word: &str, term: &str) -> bool {
    if word == term {
        return true;
    }
    let (w, t) = (word.chars().count(), term.chars().count());
    if t <= 3 {
        return false;
    }
    if word.starts_with(term) {
        return (t >= 5 && w - t <= 3) || (t == 4 && w - t <= 2);
    }
    term.starts_with(word) && w >= 6 && t - w <= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(q: &str) -> Vec<String> {
        super::super::query::words(q)
    }

    #[test]
    fn endings_and_short_words() {
        assert!(matches("iadesi", "iade"));
        assert!(matches("redondea", "redondear"));
        assert!(matches("reembolsos", "reembolso"));
        assert!(!matches("paralel", "para"));
        assert!(!matches("parametre", "para"));
        assert!(!matches("cid", "ci"));
        assert!(matches("ci", "ci"));
    }

    #[test]
    fn a_turkish_question_finds_the_english_terms() {
        let q = "KDV nasıl yuvarlanır";
        let related = related_words(q, &words(q));
        assert!(related.contains(&"vat".to_string()) && related.contains(&"rounding".to_string()), "{related:?}");
        let q = "para iadesi ne zaman yapılır";
        let related = related_words(q, &words(q));
        assert!(related.contains(&"refund".to_string()) && related.contains(&"money".to_string()), "{related:?}");
    }

    #[test]
    fn phrases_match_as_phrases() {
        let q = "reglas de la base de datos";
        assert!(related_words(q, &words(q)).contains(&"database".to_string()));
    }
}
