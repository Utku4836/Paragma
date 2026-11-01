//! What `pg run` remembers of test and build runs (in state.db), and what it
//! makes of a new one against the earlier runs of the same command: which
//! failures are new, which were failing already, which come and go (flaky),
//! which got fixed.

pub mod parse;

/// One run of a command.
#[derive(Clone, Debug, Default)]
pub struct Run {
    pub at: i64,
    pub command: String,
    pub exit: i32,
    pub seconds: f64,
    pub failing: Vec<String>,
    pub passed: usize,
    pub errors: Vec<String>,
    /// Files changed (by git) when it ran.
    pub changed: Vec<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Verdict {
    pub new: Vec<String>,
    /// Failing in this many runs in a row, this one included.
    pub already: Vec<(String, usize)>,
    pub flaky: Vec<String>,
    pub fixed: Vec<String>,
}

/// "go test ./... failed: 2 failing (TestA, TestB), 5 min ago"
pub fn describe(r: &Run) -> String {
    let when = crate::output::ago(r.at, crate::output::now());
    if r.exit == 0 {
        return format!("{} passed, {when}", r.command);
    }
    let failing = if r.failing.is_empty() {
        String::new()
    } else {
        format!(": {} failing ({})", r.failing.len(), r.failing.iter().take(3).cloned().collect::<Vec<_>>().join(", "))
    };
    format!("{} failed{failing}, {when}", r.command)
}

/// Earlier runs looked at for flaky tests.
const FLAKY_WINDOW: usize = 6;

/// `history`: earlier runs of the same command, newest first.
pub fn judge(now: &Run, history: &[Run]) -> Verdict {
    let mut v = Verdict::default();
    let last = history.first();
    for t in &now.failing {
        // failed, passed, failed: it comes and goes
        let mut states: Vec<bool> = vec![true];
        states.extend(history.iter().take(FLAKY_WINDOW).map(|r| r.failing.contains(t)));
        let changes = states.windows(2).filter(|w| w[0] != w[1]).count();
        if changes >= 2 {
            v.flaky.push(t.clone());
        } else if last.is_some_and(|r| r.failing.contains(t)) {
            let streak = 1 + history.iter().take_while(|r| r.failing.contains(t)).count();
            v.already.push((t.clone(), streak));
        } else {
            v.new.push(t.clone());
        }
    }
    if let Some(last) = last
        && (now.exit == 0 || now.passed > 0)
    {
        v.fixed = last.failing.iter().filter(|t| !now.failing.contains(t)).cloned().collect();
    }
    v
}

/// Tests that newly broke in earlier runs while these files were changed:
/// the tests a change to them tends to break.
pub fn broken_by(runs: &[Run], files: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // runs are newest first: compare each with the run before it
    for (i, r) in runs.iter().enumerate() {
        let before = runs.get(i + 1);
        if !r.changed.iter().any(|c| files.contains(c)) {
            continue;
        }
        for t in &r.failing {
            let new = before.is_none_or(|b| b.command == r.command && !b.failing.contains(t));
            if new && !out.contains(t) {
                out.push(t.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(failing: &[&str], exit: i32) -> Run {
        Run { failing: failing.iter().map(|s| s.to_string()).collect(), exit, passed: 3, ..Default::default() }
    }

    #[test]
    fn new_already_flaky_fixed() {
        let history = vec![run(&["A", "C"], 1), run(&["A"], 1), run(&["C"], 1)];
        let v = judge(&run(&["A", "B", "C"], 1), &history);
        assert_eq!(v.new, vec!["B"]);
        assert_eq!(v.already, vec![("A".to_string(), 3)]);
        assert_eq!(v.flaky, vec!["C"]);
        let v = judge(&run(&[], 0), &history);
        assert_eq!(v.fixed, vec!["A", "C"]);
    }

    #[test]
    fn tests_broken_by_a_file() {
        let mut r1 = run(&["TestRefund"], 1);
        r1.changed = vec!["payments/refund.go".into()];
        let r0 = run(&[], 0);
        assert_eq!(broken_by(&[r1, r0], &["payments/refund.go".into()]), vec!["TestRefund"]);
    }
}
