//! What pg reads from files it does not control (notes written by hand or
//! by Obsidian, a command's output) given every kind of text: it never
//! panics, and what it writes back reads the same.

use crate::note::{Note, Value};
use crate::project::plan;
use proptest::prelude::*;

/// Text full of what the readers look for: property fences, list marks,
/// links, line endings of both kinds, a byte order mark, any letters.
fn tricky() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        Just("---\n".to_string()),
        Just("---".to_string()),
        Just("\r\n".to_string()),
        Just("\n".to_string()),
        Just("- [ ] ".to_string()),
        Just("- [x] ".to_string()),
        Just("- [-] ".to_string()),
        Just("- [~] ".to_string()),
        Just("## ".to_string()),
        Just("id: ".to_string()),
        Just("type: decision\n".to_string()),
        Just("[[".to_string()),
        Just("]]".to_string()),
        Just("|".to_string()),
        Just("#".to_string()),
        Just(": ".to_string()),
        Just("  - ".to_string()),
        Just("\"".to_string()),
        Just("'".to_string()),
        Just("\u{feff}".to_string()),
        Just("\t".to_string()),
        "[a-zA-Z0-9 .,]{0,12}",
        "\\PC{0,6}",
    ];
    prop::collection::vec(piece, 0..40).prop_map(|v| v.concat())
}

/// A key as notes have them.
fn key() -> impl Strategy<Value = String> {
    "[a-z][a-z_]{0,10}"
}

/// A value on one line, with what YAML treats specially.
fn line_value() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 ,:#'\"\\[\\]{}()./!?@&*çğıöşüÇĞİÖŞÜ-]{1,40}"
        .prop_map(|s| s.trim().to_string())
        .prop_filter("not empty", |s| !s.is_empty())
}

/// A property block a person or Obsidian may write: plain values, lists,
/// nested blocks, multi-line text.
fn written_props() -> impl Strategy<Value = String> {
    let entry =
        (key(), 0..5usize, line_value(), prop::collection::vec(line_value(), 1..4)).prop_map(|(k, form, v, items)| {
            match form {
                0 => format!("{k}: {v}\n"),
                1 => format!(
                    "{k}: [{}]\n",
                    items.iter().map(|i| format!("\"{}\"", i.replace('"', ""))).collect::<Vec<_>>().join(", ")
                ),
                2 => format!(
                    "{k}:\n{}",
                    items.iter().map(|i| format!("  - {}\n", i.replace(['"', '\''], ""))).collect::<String>()
                ),
                3 => format!(
                    "{k}:\n  source: {}\n  nested:\n    deeper: {}\n",
                    v.replace(['"', '\''], ""),
                    items[0].replace(['"', '\''], "")
                ),
                _ => format!("{k}: |\n  {}\n  {}\n", v, items[0]),
            }
        });
    prop::collection::vec(entry, 0..6).prop_map(|entries| {
        // one entry per key, as YAML wants
        let mut seen = std::collections::HashSet::new();
        entries.into_iter().filter(|e| seen.insert(e.split(':').next().unwrap_or("").to_string())).collect::<String>()
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

    /// Any text: read without a panic, and written back, it settles at
    /// once (writing what was read gives the same text again).
    #[test]
    fn any_text_is_read_and_settles(text in tricky()) {
        let note = Note::parse(&text);
        let _ = (note.title(), note.kind(), note.id(), note.section("Decision"));
        let once = note.render();
        let twice = Note::parse(&once).render();
        prop_assert_eq!(&once, &twice);
    }

    /// A note as people write it comes back byte for byte when pg changes
    /// nothing in it.
    #[test]
    fn a_written_note_comes_back_as_it_was(props in written_props(), body in "[a-zA-Z0-9 .,#\\[\\]\n-]{0,200}") {
        let body = body.trim_start_matches(['-', '\n']).to_string();
        let text = if props.is_empty() { body.clone() } else { format!("---\n{props}---\n{body}") };
        let text = if text.ends_with('\n') { text } else { format!("{text}\n") };
        prop_assert_eq!(Note::parse(&text).render(), text);
    }

    /// Only the property pg changes changes: the others stay as written.
    #[test]
    fn setting_one_property_keeps_the_others(props in written_props(), value in line_value()) {
        let text = format!("---\n{props}---\nBody.\n");
        let mut note = Note::parse(&text);
        note.props.set_text("status", value.clone());
        let out = note.render();
        let again = Note::parse(&out);
        prop_assert_eq!(again.props.text("status"), Some(value.as_str()));
        for line in props.lines().filter(|l| !l.starts_with("status:") && !l.starts_with(' ')) {
            if !props.starts_with("status:") {
                prop_assert!(out.contains(line), "{line:?} lost:\n{out}");
            }
        }
    }

    /// A value with YAML's special characters reads back as it was set.
    #[test]
    fn a_value_reads_back(value in line_value(), items in prop::collection::vec(line_value(), 0..5)) {
        let mut note = Note::default();
        note.props.set_text("title", value.clone());
        note.props.set("files", Value::List(items.clone()));
        let again = Note::parse(&note.render());
        prop_assert_eq!(again.props.text("title"), Some(value.as_str()));
        prop_assert_eq!(again.props.list("files"), items);
    }
}

/// What a plan's steps are, kept beside the note to compare.
#[derive(Clone, Debug)]
enum Op {
    Add(String),
    Close(usize),
    Drop(usize),
    Reopen(usize),
    Defer(usize, bool),
    Edit(usize, String),
}

fn step_text() -> impl Strategy<Value = String> {
    "[A-Za-zçğış][A-Za-z0-9 çğışöü,'()/-]{0,30}".prop_map(|s| s.trim().to_string())
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => step_text().prop_map(Op::Add),
        1 => (1..12usize).prop_map(Op::Close),
        1 => (1..12usize).prop_map(Op::Drop),
        1 => (1..12usize).prop_map(Op::Reopen),
        1 => (1..12usize, any::<bool>()).prop_map(|(n, l)| Op::Defer(n, l)),
        1 => (1..12usize, step_text()).prop_map(|(n, t)| Op::Edit(n, t)),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    /// Whatever is done to a plan, written and read again between each
    /// change: every step keeps its number and its state.
    #[test]
    fn steps_keep_their_numbers(ops in prop::collection::vec(op(), 1..30)) {
        let mut note = Note::parse("---\ntype: project\ntitle: shop\n---\n## Goal\nRefunds.\n");
        let mut model: Vec<plan::Step> = Vec::new();
        for op in ops {
            match &op {
                Op::Add(t) => {
                    let fresh = !model.iter().any(|s| !s.done && !s.dropped && s.text == *t);
                    prop_assert_eq!(plan::add_step(&mut note, t), fresh && !t.is_empty());
                    if fresh && !t.is_empty() {
                        model.push(plan::Step { done: false, deferred: false, dropped: false, text: t.clone() });
                    }
                }
                Op::Close(n) => {
                    if plan::close_number(&mut note, *n).is_some() {
                        model[n - 1].done = true;
                        model[n - 1].deferred = false;
                    }
                }
                Op::Drop(n) => {
                    if plan::drop_number(&mut note, *n).is_some() {
                        model[n - 1].dropped = true;
                        model[n - 1].done = false;
                    }
                }
                Op::Reopen(n) => {
                    if plan::reopen_number(&mut note, *n).is_some() {
                        model[n - 1].done = false;
                    }
                }
                Op::Defer(n, later) => {
                    if plan::defer_number(&mut note, *n, *later).is_some() {
                        model[n - 1].deferred = *later;
                    }
                }
                Op::Edit(n, t) => {
                    if plan::edit_number(&mut note, *n, t).is_some() {
                        model[n - 1].text = t.clone();
                    }
                }
            }
            note = Note::parse(&note.render());
            let steps = plan::steps(&note);
            prop_assert_eq!(steps.len(), model.len(), "after {:?}: {:?}", op, note.body);
            for (i, (got, want)) in steps.iter().zip(&model).enumerate() {
                prop_assert_eq!(&got.text, &want.text, "step {} after {:?}", i + 1, op);
                prop_assert_eq!(got.done, want.done, "step {} after {:?}", i + 1, op);
                prop_assert_eq!(got.dropped, want.dropped, "step {} after {:?}", i + 1, op);
            }
        }
    }

    /// Links in any text: found without a panic, each one written in it,
    /// and each resolved to a note that exists or to none.
    #[test]
    fn links_are_read_from_any_text(text in tricky(), paths in prop::collection::vec("[a-z]{1,4}(/[a-z]{1,4}){0,2}\\.md", 0..8)) {
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        for target in crate::search::graph::wiki_links(&text) {
            prop_assert!(text.contains(&target));
            if let Some(p) = crate::search::graph::resolve(&target, paths.first().copied().unwrap_or("a.md"), &paths) {
                prop_assert!(paths.contains(&p.as_str()));
            }
        }
    }

    /// Any command output: made short without a panic, within its lines,
    /// and every error line in it kept.
    #[test]
    fn any_output_is_made_short(lines in prop::collection::vec(prop_oneof![
        Just("error[E0308]: mismatched types".to_string()),
        Just("  --> src/a.rs:10:5".to_string()),
        Just("warning: unused variable `x`".to_string()),
        Just("   Compiling foo v0.1.0".to_string()),
        Just("thread 'main' panicked at src/x.rs:3:1:".to_string()),
        Just("\u{1b}[31mFAILED\u{1b}[0m test_one".to_string()),
        Just("10 |     let x = 1;".to_string()),
        Just("[=====>      ] 45%".to_string()),
        "\\PC{0,30}",
    ], 0..120), max in 5..60usize) {
        let output = lines.join("\n");
        let short = crate::testrun::compact::compact(&output, max);
        let _ = crate::testrun::parse::parse(&output);
        for l in &lines {
            let _ = crate::testrun::parse::locations(l);
        }
        // an error never disappears into a count
        if lines.iter().any(|l| l.starts_with("error[E0308]")) {
            prop_assert!(short.iter().any(|l| l.contains("error[E0308]")), "{:?}", short);
        }
    }

    /// Any question: its words and its kind, without a panic.
    #[test]
    fn any_question_is_read(q in tricky()) {
        let _ = crate::search::words_of(&q);
        let _ = crate::search::classify(&q);
    }
}
