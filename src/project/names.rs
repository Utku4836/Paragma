//! Whether a name given to `pg open` means a project that already exists.
//!
//! Two jobs pulling in opposite directions: an agent in a new conversation
//! should land in the project it worked on before even when it writes the
//! name differently, and two different pieces of work must never be merged
//! by accident (splitting them later is harder than merging two projects
//! with `pg mv`). So only the same name written another way, or a small
//! typo, continues a project; a shared word only gets a mention.

/// What a name means among the existing projects.
#[derive(Debug, PartialEq)]
pub enum Match {
    /// The same name written another way: "Cart Discounts", "cart_discounts".
    Same(String),
    /// A small typo of an existing name: "shopfy-magazasi".
    Close(String),
    /// Different names sharing a word ("discounts" and "cart-discounts"):
    /// a new project, but worth a mention.
    Similar(Vec<String>),
    None,
}

/// Words that name "the project here" rather than a project.
pub const GENERIC: &[&str] = &["repo", "project", "proje", "this", "here", "app", "current", "workspace", "code"];

/// The name a new project gets: lower case, words joined by '-'.
pub fn slug(name: &str) -> String {
    name.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-")
}

pub fn match_name(name: &str, existing: &[String]) -> Match {
    if let Some(e) = existing.iter().find(|e| *e == name) {
        return Match::Same(e.clone());
    }
    let key = key(name);
    if key.is_empty() {
        return Match::None;
    }
    if let Some(e) = existing.iter().find(|e| self::key(e) == key) {
        return Match::Same(e.clone());
    }
    let close: Vec<&String> = existing
        .iter()
        .filter(|e| {
            let k = self::key(e);
            let allowed = if k.chars().count().max(key.chars().count()) <= 8 { 1 } else { 2 };
            key.chars().count() >= 5 && distance(&k, &key) <= allowed
        })
        .collect();
    if let [one] = close.as_slice() {
        return Match::Close((*one).clone());
    }
    let mine = words(name);
    let similar: Vec<String> = existing
        .iter()
        .filter(|e| words(e).iter().any(|w| w.chars().count() >= 4 && mine.contains(w)))
        .cloned()
        .collect();
    if similar.is_empty() { Match::None } else { Match::Similar(similar) }
}

/// Lower case, Turkish letters as ASCII, plural endings off.
fn words(name: &str) -> Vec<String> {
    let folded: String = name
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'ı' | 'î' => 'i',
            'ş' => 's',
            'ğ' => 'g',
            'ü' | 'û' => 'u',
            'ö' => 'o',
            'ç' => 'c',
            'â' => 'a',
            c => c,
        })
        .collect();
    folded.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(singular).collect()
}

fn singular(w: &str) -> String {
    let n = w.chars().count();
    for end in ["lar", "ler"] {
        if n > 5 && w.ends_with(end) {
            return w[..w.len() - 3].to_string();
        }
    }
    if n > 3 && w.ends_with('s') && !w.ends_with("ss") {
        return w[..w.len() - 1].to_string();
    }
    w.to_string()
}

/// A name's words in a fixed order, so word order does not matter.
fn key(name: &str) -> String {
    let mut w = words(name);
    w.sort();
    w.join("-")
}

/// Edit distance (insertions, deletions, substitutions).
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != b[j])).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing() -> Vec<String> {
        ["cart-discounts", "shopify-magazasi", "game-engine", "api"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_same_name_written_another_way() {
        assert_eq!(match_name("Cart Discounts", &existing()), Match::Same("cart-discounts".into()));
        assert_eq!(match_name("cart_discount", &existing()), Match::Same("cart-discounts".into()));
        assert_eq!(match_name("discounts cart", &existing()), Match::Same("cart-discounts".into()));
        assert_eq!(match_name("Shopify Mağazası", &existing()), Match::Same("shopify-magazasi".into()));
    }

    #[test]
    fn typos_continue_but_shared_words_do_not() {
        assert_eq!(match_name("shopfy-magazasi", &existing()), Match::Close("shopify-magazasi".into()));
        assert_eq!(match_name("discounts", &existing()), Match::Similar(vec!["cart-discounts".into()]));
        assert_eq!(match_name("oyun-motoru", &existing()), Match::None);
        // short names are too easy to confuse
        assert_eq!(match_name("apj", &existing()), Match::None);
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Cart Discounts"), "cart-discounts");
        assert_eq!(slug("Oyun Motoru!"), "oyun-motoru");
    }
}
