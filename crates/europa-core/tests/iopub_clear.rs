mod common;

use common::{clear, notebook, outputs, running, stream};
use europa_core::{apply_iopub, ExecState, IopubMsg, MsgId, Notebook};

/// A running `c1` (`m1`) that already printed `a\n`.
fn printed() -> (ExecState, Notebook) {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let _ = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", "a\n"));
    (exec, book)
}

fn m1() -> MsgId {
    MsgId::new("m1")
}

#[test]
fn immediate_clear_empties_outputs() {
    let (mut exec, mut book) = printed();
    let applied = apply_iopub(&mut exec, &mut book, &clear("m1", false));
    assert_eq!(applied.as_deref(), Some("c1"));
    assert_eq!(outputs(&book, "c1"), "");
}

#[test]
fn waiting_clear_keeps_outputs_and_stays_pending() {
    let (mut exec, mut book) = printed();
    let _ = apply_iopub(&mut exec, &mut book, &clear("m1", true));
    assert_eq!(outputs(&book, "c1"), r#"stdout:"a\n""#);
    assert!(exec.is_clear_pending(&m1()));
}

#[test]
fn next_output_clears_first_and_ends_the_pending_state() {
    let (mut exec, mut book) = printed();
    let _ = apply_iopub(&mut exec, &mut book, &clear("m1", true));
    let _ = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", "b\n"));
    assert_eq!(outputs(&book, "c1"), r#"stdout:"b\n""#);
    assert!(!exec.is_clear_pending(&m1()));
}

#[test]
fn repeated_waiting_clear_is_one_pending_entry() {
    let (mut exec, mut book) = printed();
    let _ = apply_iopub(&mut exec, &mut book, &clear("m1", true));
    let _ = apply_iopub(&mut exec, &mut book, &clear("m1", true));
    assert_eq!(outputs(&book, "c1"), r#"stdout:"a\n""#);
    let json = serde_json::to_value(&exec).expect("serializes");
    assert_eq!(json["pending_clear"].as_array().map(Vec::len), Some(1));
}

#[test]
fn immediate_clear_resolves_a_pending_one() {
    let (mut exec, mut book) = printed();
    let _ = apply_iopub(&mut exec, &mut book, &clear("m1", true));
    let _ = apply_iopub(&mut exec, &mut book, &clear("m1", false));
    assert_eq!(outputs(&book, "c1"), "");
    assert!(!exec.is_clear_pending(&m1()));
}

#[test]
fn empty_stream_does_not_resolve_a_pending_clear() {
    let (mut exec, mut book) = printed();
    let _ = apply_iopub(&mut exec, &mut book, &clear("m1", true));
    let _ = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", ""));
    assert_eq!(outputs(&book, "c1"), r#"stdout:"a\n""#);
    assert!(exec.is_clear_pending(&m1()));
}

#[test]
fn only_an_immediate_clear_takes_outputs_from_some_to_none() {
    let (mut exec, mut book) = printed();
    let steps: Vec<(IopubMsg, bool)> = vec![
        (clear("m1", true), false),
        (stream("m1", "stdout", "x\n"), false),
        (clear("m1", true), false),
        (clear("m1", true), false),
        (stream("m1", "stdout", "y\n"), false),
        (clear("m1", false), true),
        (stream("m1", "stdout", "z\n"), false),
        (clear("m1", true), false),
    ];
    for (msg, empties) in steps {
        let before = outputs_len(&book);
        let _ = apply_iopub(&mut exec, &mut book, &msg);
        let after = outputs_len(&book);
        let emptied = before >= 1 && after == 0;
        assert_eq!(emptied, empties, "{msg:?}: {before} -> {after}");
    }
}

fn outputs_len(book: &Notebook) -> usize {
    let text = outputs(book, "c1");
    if text.is_empty() {
        0
    } else {
        text.split(", ").count()
    }
}
