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

/// Seconds since the epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// A time as a UTC date: 2026-11-01.
pub fn date(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{}-{m:02}-{d:02}", if m <= 2 { y + 1 } else { y })
}

/// "2026-11-01" as seconds since the epoch (UTC midnight).
pub fn parse_date(s: &str) -> Option<i64> {
    let mut parts = s.trim().get(..10)?.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // days from civil (Howard Hinnant's algorithm)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400)
}

/// A time as a UTC date and minute: 2026-11-01 14:10Z.
pub fn date_time(secs: i64) -> String {
    let s = secs.rem_euclid(86_400);
    format!("{} {:02}:{:02}Z", date(secs), s / 3600, s % 3600 / 60)
}

/// How long ago, for people and agents: "5 min ago", "3 h ago", "2 days ago".
pub fn ago(then: i64, now: i64) -> String {
    let d = (now - then).max(0);
    match d {
        0..60 => "just now".to_string(),
        60..3600 => format!("{} min ago", d / 60),
        3600..86_400 => format!("{} h ago", d / 3600),
        86_400..172_800 => "yesterday".to_string(),
        _ if d < 30 * 86_400 => format!("{} days ago", d / 86_400),
        _ => format!("on {}", date(then)),
    }
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
    fn dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_792_000_000), "2026-10-14");
        assert_eq!(parse_date("2026-10-14"), Some(1_792_000_000 - 1_792_000_000 % 86_400));
        assert_eq!(parse_date("1970-01-01"), Some(0));
        assert_eq!(parse_date("soon"), None);
        assert_eq!(date_time(1_792_000_000 + 3 * 3600 + 120), "2026-10-14 20:48Z");
        assert_eq!(ago(100, 100 + 7200), "2 h ago");
        assert_eq!(ago(0, 100 * 86_400), "on 1970-01-01");
    }

    #[test]
    fn one_line_skips_headings() {
        assert_eq!(one_line("## Fix\nNormalise line endings.\nMore.", 100), "Normalise line endings.");
        assert_eq!(one_line("abcdefghij", 4), "abcd...");
    }
}

/// With $PG_PROFILE set, how long pg has been running at a point of its
/// work, on stderr: to see where the time goes.
pub fn mark(what: &str) {
    static START: std::sync::OnceLock<(std::time::Instant, bool)> = std::sync::OnceLock::new();
    let (start, on) = START.get_or_init(|| (std::time::Instant::now(), std::env::var_os("PG_PROFILE").is_some()));
    if *on {
        eprintln!("profile {:7.1} ms  {what}", start.elapsed().as_secs_f64() * 1000.0);
    }
}
