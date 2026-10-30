//! Output for agents: plain text within a token budget.

/// Tokens a text costs, roughly (3.5 characters per token).
pub fn tokens(s: &str) -> usize {
    (s.chars().count() * 2).div_ceil(7)
}

/// Collects output until the budget is spent.
pub struct Budget {
    left: usize,
    pub out: String,
}

impl Budget {
    pub fn new(tokens: usize) -> Budget {
        Budget { left: tokens, out: String::new() }
    }

    /// Adds the block if it fits; false when it does not.
    pub fn add(&mut self, block: &str) -> bool {
        let t = tokens(block);
        if t > self.left {
            return false;
        }
        self.left -= t;
        self.out.push_str(block);
        if !block.ends_with('\n') {
            self.out.push('\n');
        }
        true
    }
}

/// "1 note", "2 notes".
pub fn count(n: impl Into<i64>, what: &str) -> String {
    let n = n.into();
    if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") }
}

/// The first line of a text, cut to about `max` characters.
pub fn one_line(s: &str, max: usize) -> String {
    let line = s.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).unwrap_or("");
    if line.chars().count() <= max {
        return line.to_string();
    }
    let cut: String = line.chars().take(max).collect();
    format!("{}...", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_stops_when_full() {
        let mut b = Budget::new(10);
        assert!(b.add("short"));
        assert!(!b.add(&"x".repeat(100)));
        assert_eq!(b.out, "short\n");
    }

    #[test]
    fn counts() {
        assert_eq!(count(1, "note"), "1 note");
        assert_eq!(count(0, "note"), "0 notes");
    }

    #[test]
    fn one_line_skips_headings() {
        assert_eq!(one_line("## Fix\nNormalise line endings.\nMore.", 100), "Normalise line endings.");
        assert_eq!(one_line("abcdefghij", 4), "abcd...");
    }
}
