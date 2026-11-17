//! What explains an error line in a command's output: the lines after it
//! that say what was expected and what came (`left:`/`right:`, `Expected`/
//! `Received`, an assertion's message), with long values cut to where they
//! differ, and the frames of a stack trace that are the project's own (a
//! library's frames said as a count). And where in the whole output each
//! error is, for reading just that part.

/// Lines of explanation kept for one error.
const DETAILS: usize = 5;
/// A value longer than this is cut to where it differs from its pair.
const LONG: usize = 80;
/// Characters kept around the first difference.
const AROUND: usize = 30;

/// The 1-based line of the output where `line` (an error line as parsed)
/// is, and the lines that explain it.
pub fn context(output: &str, line: &str) -> (Option<usize>, Vec<String>) {
    let lines: Vec<&str> = output.lines().collect();
    // the parsed line may have "--> place" added, and is cut at 300 characters
    let head: String = line.split(" --> ").next().unwrap_or(line).trim().chars().take(120).collect();
    let Some(at) = lines.iter().position(|l| !head.is_empty() && l.trim().starts_with(&head)) else {
        return (None, Vec::new());
    };
    let mut details: Vec<String> = Vec::new();
    let mut library = 0usize;
    // Rust says a panic's message on the lines after "panicked at", as they are
    let panic = head.contains("panicked at");
    for l in lines.iter().skip(at + 1) {
        let t = l.trim();
        if t.is_empty() || ends_block(t) {
            break;
        }
        if t.starts_with("note: run with `RUST_BACKTRACE") {
            continue; // the same advice under every panic
        }
        if library_frame(t) {
            library += 1;
            continue;
        }
        if panic || explains(l) {
            if library > 0 {
                details.push(format!("... {}", crate::output::count(library as i64, "library frame")));
                library = 0;
            }
            details.push(t.chars().take(300).collect());
        } else {
            break;
        }
        if details.len() >= DETAILS {
            break;
        }
    }
    if library > 0 && details.len() < DETAILS {
        details.push(format!("... {}", crate::output::count(library as i64, "library frame")));
    }
    (Some(at + 1), shorten_pairs(details))
}

/// A line that starts something else: the next test, error or summary.
fn ends_block(t: &str) -> bool {
    let lower = t.to_lowercase();
    t.starts_with("--- ")
        || t.starts_with("test ")
        || t.starts_with("thread '")
        || lower.starts_with("error")
        || lower.starts_with("failures:")
        || lower.starts_with("fail")
        || lower.starts_with("ok ")
        || t.starts_with("===")
}

/// A line that belongs to the error above it: indented, a value pair, an
/// assertion's words, a frame, a note.
fn explains(l: &str) -> bool {
    let t = l.trim_start();
    let lower = t.to_lowercase();
    l.starts_with(char::is_whitespace)
        || ["left:", "right:", "expected", "received", "actual", "got", "want", "assertion", "note:", "help:", "at "]
            .iter()
            .any(|p| lower.starts_with(p))
        || lower.starts_with("e  ") // pytest's own explanation lines
        || t.starts_with('>')
        || t.starts_with('|')
}

/// A frame of a stack trace in someone else's code.
pub fn library_frame(t: &str) -> bool {
    let numbered = t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains(": ");
    let frame = t.starts_with("at ") || t.starts_with("File \"") || numbered;
    frame
        && [
            "node_modules",
            "site-packages",
            "dist-packages",
            "/rustc/",
            "\\rustc\\",
            ".cargo/registry",
            ".cargo\\registry",
            "<frozen",
            "node:internal",
            "internal/",
            "/usr/lib/",
            "lib/python",
            "core::panicking",
            "std::",
            "core::",
        ]
        .iter()
        .any(|m| t.contains(m))
}

/// "left: <long>" and "right: <long>" (or expected/received, want/got):
/// both cut to where they first differ.
fn shorten_pairs(mut lines: Vec<String>) -> Vec<String> {
    let value = |l: &str| -> Option<(usize, usize)> {
        let lower = l.to_lowercase();
        ["left:", "right:", "expected:", "received:", "actual:", "got:", "want:", "expected", "received"]
            .iter()
            .find(|p| lower.starts_with(*p))
            .map(|p| (p.len(), l.len()))
    };
    let mut i = 0;
    while i + 1 < lines.len() {
        if let (Some((a, _)), Some((b, _))) = (value(&lines[i]), value(&lines[i + 1])) {
            let (x, y) = (lines[i][a..].trim().to_string(), lines[i + 1][b..].trim().to_string());
            if x.chars().count() > LONG || y.chars().count() > LONG {
                let (sx, sy) = differ(&x, &y);
                lines[i] = format!("{} {sx}", lines[i][..a].trim_end());
                lines[i + 1] = format!("{} {sy}", lines[i + 1][..b].trim_end());
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    lines
}

/// Two values cut to the part around their first difference.
pub fn differ(a: &str, b: &str) -> (String, String) {
    let (ac, bc): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let same = ac.iter().zip(&bc).take_while(|(x, y)| x == y).count();
    let from = same.saturating_sub(AROUND);
    let cut = |c: &[char]| {
        let to = (same + AROUND).min(c.len());
        let mut s = String::new();
        if from > 0 {
            s.push_str(&format!("…({from} same) "));
        }
        s.extend(&c[from.min(c.len())..to]);
        if to < c.len() {
            s.push_str(&format!(" …(+{})", c.len() - to));
        }
        s
    };
    (cut(&ac), cut(&bc))
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/testrun/explain.rs"]
mod tests;
