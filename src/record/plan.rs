//! Whether a user's message gives decisions or a plan of several steps for
//! work that a later conversation needs ("My decisions: ... Plan: (a) ...
//! (b) ... Do only (a) now", "Kararlarım: ..."), so the agent can be asked
//! to record them before it starts: the next conversation knows only what
//! was recorded. Like said_rule.rs this finds candidates and would rather
//! miss one than call an ordinary request a plan.
//!
//! A message is a candidate when it names decisions (decided, agreed, my
//! decisions, kararlarım) or a plan (plan, roadmap, steps, adımlar), or
//! when it lists two or more steps (1. ... 2. ..., (a) ... (b) ..., first ...
//! then ...) and limits what is done now (only step 1, for now, today, in
//! another conversation; sadece, şimdilik, bugün, başka bir konuşmada).
//! Lists that report something (steps to reproduce, failing tests) and
//! questions are not plans.

use regex::Regex;
use std::sync::LazyLock;

/// Words that name decisions or a plan.
static PLAN_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        \b(my\s+decisions|decisions\s*(:|so\s+far)|we\s+(have\s+)?(decided|agreed)|i\s+(have\s+)?decided|agreed\s+on
          |my\s+plan|the\s+plan|plan\s*(:|for\b)|roadmap|into\s+steps|steps\s*:|\w+\s+steps\s*:|rules\s+of\s+the\s+game)
        |karar(lar)?(ım|ımız|ımı|ımızı|ı|ları|lar)?\b|karar\s+verdi|planım|plan\s*:|plan\s+şu|adımlar\s*:|yol\s+haritası",
    )
    .expect("pattern")
});

/// The items of a list of steps: "1." "2)" "(a)" "b)" "- ".
static ITEMS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)(?:^|[\s:])(?:\d{1,2}[.)]|\([a-z0-9]\)|[a-e]\)|-)\s+\S").expect("pattern"));

/// first ... then ..., önce ... sonra ...
static SEQUENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(first\b.{3,120}\bthen\b|önce\b.{3,120}\bsonra\b)").expect("pattern"));

/// What limits the work done now, leaving the rest for later.
static NOW_ONLY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        \b(do\s+only|only\s+(step|the|\(|\d|[a-e]\b)|step\s+\d+\s+only|\(\w\)\s+only|for\s+now|today|this\s+session
          |in\s+this\s+conversation|another\s+conversation|next\s+conversation|later\s+sessions?|next\s+week|tomorrow
          |the\s+rest|the\s+others|first\s+step|start\s+with)
        |\b(sadece|yalnızca|şimdilik|bugün|bu\s+konuşmada|başka\s+bir\s+konuşma|sonraki\s+konuşma|kalan(ı|ını)|sonra\s+konuşuruz|yeterli)",
    )
    .expect("pattern")
});

/// Lists that report, not plan.
static REPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)steps\s+to\s+reproduce|\bfail(s|ed|ing)?\s+with\b|\berror\b|\bhata\b|\bpanic\b").expect("pattern")
});

/// Whether the message gives decisions or a plan worth recording.
pub fn gives_plan(message: &str) -> bool {
    let m = message.trim();
    if m.len() < 40 {
        return false;
    }
    if PLAN_WORDS.is_match(m) {
        return true;
    }
    if REPORT.is_match(m) || m.ends_with('?') {
        return false;
    }
    let steps = ITEMS.find_iter(m).count() >= 2 || SEQUENCE.is_match(m);
    steps && NOW_ONLY.is_match(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(split: &str) -> Vec<(bool, String)> {
        include_str!("../../bench/prompt-plans.tsv")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let mut p = l.splitn(3, '\t');
                let (s, label, msg) = (p.next()?, p.next()?, p.next()?);
                (s == split).then(|| (label == "plan", msg.to_string()))
            })
            .collect()
    }

    #[test]
    fn the_messages_looked_at_while_building() {
        let wrong: Vec<String> = set("dev")
            .into_iter()
            .filter(|(want, m)| gives_plan(m) != *want)
            .map(|(want, m)| format!("{} {m}", if want { "missed" } else { "false alarm" }))
            .collect();
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// The held messages, measured once: SPLIT=held cargo test measure -- --ignored --nocapture
    #[test]
    #[ignore]
    fn measure() {
        let split = std::env::var("SPLIT").unwrap_or_else(|_| "dev".into());
        let rows = set(&split);
        let (mut hit, mut plans, mut alarms, mut others) = (0, 0, 0, 0);
        for (want, m) in &rows {
            let got = gives_plan(m);
            if *want {
                plans += 1;
                hit += usize::from(got);
            } else {
                others += 1;
                alarms += usize::from(got);
            }
            if got != *want {
                println!("{} {m}", if *want { "missed" } else { "false alarm" });
            }
        }
        println!("{split}: found {hit} of {plans} plans, {alarms} false alarms in {others} other messages");
    }
}
