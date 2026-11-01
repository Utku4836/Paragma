//! An error line's signature: the line with what changes from one run to
//! the next taken out (numbers, hashes, ids, folders), so the same error
//! seen again is known as the same.
//!
//! ```text
//! C:\dev\shop\catalog\catalog_test.go:8: checksum mismatch: got 8c77cb095cbd want 4ff52f25fc9d
//! catalog_test.go:<n>: checksum mismatch: got <hex> want <hex>
//! ```

/// The signature of the first non-empty line of `text`.
pub fn normalize(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    line.split_whitespace().map(word).collect::<Vec<_>>().join(" ")
}

/// What a signature says, without where (file:line) and without the
/// marks: what two errors are compared by.
pub fn message(signature: &str) -> String {
    signature
        .split_whitespace()
        .filter(|w| !(w.contains(':') && w.contains('.') && w.contains("<n>")))
        .map(|w| w.replace("<n>", "").replace("<hex>", "").replace("<id>", ""))
        .filter(|w| !w.trim_matches(|c: char| !c.is_alphanumeric()).is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn word(w: &str) -> String {
    // a path: only the file name says something about the error
    let w = if w.matches(['/', '\\']).count() >= 2 { w.rsplit(['/', '\\']).next().unwrap_or(w) } else { w };
    let mut out = String::new();
    let mut run = String::new();
    for c in w.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() {
            run.push(c);
            continue;
        }
        out.push_str(&changing(&run));
        run.clear();
        if c != ' ' {
            out.push(c);
        }
    }
    out
}

/// A run of letters and digits that changes between runs becomes a mark.
fn changing(run: &str) -> String {
    let digits = run.chars().filter(char::is_ascii_digit).count();
    if run.is_empty() || digits == 0 {
        return run.to_string();
    }
    if digits == run.chars().count() {
        return "<n>".into();
    }
    if run.chars().count() >= 6 && run.chars().all(|c| c.is_ascii_hexdigit()) {
        return "<hex>".into();
    }
    if run.chars().count() >= 16 {
        return "<id>".into(); // uuids without dashes, ULIDs, tokens
    }
    run.to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn what_changes_between_runs_is_taken_out() {
        assert_eq!(
            normalize(
                "C:\\dev\\shop\\catalog\\catalog_test.go:8: catalog checksum mismatch: got 8c77cb095cbd want 4ff52f25fc9d"
            ),
            "catalog_test.go:<n>: catalog checksum mismatch: got <hex> want <hex>"
        );
        assert_eq!(normalize("\n  database is locked (5)\nmore"), "database is locked (<n>)");
        assert_eq!(normalize("order 01JBX3K9Q8ZJ4N6V2T7R5M1C0D not found"), "order <id> not found");
        assert_eq!(normalize("utf8 decode failed in main.rs"), "utf8 decode failed in main.rs");
        assert_eq!(super::message("refund_test.go:<n>: window: got <n>"), "window: got");
    }
}
