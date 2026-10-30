mod common;

use common::{clear, notebook, outputs, running, stream};
use europa_core::{apply_iopub, ExecEvent, ExecState, IopubMsg, MsgId, Notebook, ReplyStatus};

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
    assert_eq!(applied.map(|applied| applied.cell), Some("c1".to_owned()));
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

fn pending() -> (ExecState, Notebook) {
    let (mut exec, mut book) = printed();
    let _ = apply_iopub(&mut exec, &mut book, &clear("m1", true));
    (exec, book)
}

fn reply_ok() -> ExecEvent {
    ExecEvent::Reply {
        parent: m1(),
        status: ReplyStatus::Ok,
    }
}

fn idle() -> ExecEvent {
    ExecEvent::Idle { parent: m1() }
}

#[test]
fn ending_a_run_drops_the_pending_clear_and_keeps_outputs() {
    let (mut exec, book) = pending();
    let _ = exec.apply(reply_ok());
    let _ = exec.apply(idle());
    assert_eq!(outputs(&book, "c1"), r#"stdout:"a\n""#);
    assert!(!exec.is_clear_pending(&m1()));
}

#[test]
fn pending_clear_survives_until_both_idle_and_reply_are_seen() {
    let (mut exec, _book) = pending();
    let _ = exec.apply(idle());
    assert!(exec.is_clear_pending(&m1()));
    let _ = exec.apply(reply_ok());
    assert!(!exec.is_clear_pending(&m1()));
}

#[test]
fn reset_drops_the_pending_clear() {
    let (mut exec, _book) = pending();
    let _ = exec.apply(ExecEvent::Reset);
    assert!(!exec.is_clear_pending(&m1()));
    let text = serde_json::to_string(&exec).expect("serializes");
    serde_json::from_str::<ExecState>(&text).expect("still valid");
}

#[test]
fn removing_the_cell_drops_the_pending_clear() {
    let (mut exec, _book) = pending();
    let _ = exec.apply(ExecEvent::RemoveCell {
        cell_id: "c1".to_owned(),
    });
    assert!(!exec.is_clear_pending(&m1()));
    let text = serde_json::to_string(&exec).expect("serializes");
    serde_json::from_str::<ExecState>(&text).expect("still valid");
}

#[test]
fn a_rerun_is_not_cleared_by_the_previous_runs_pending_clear() {
    let (mut exec, mut book) = pending();
    let _ = exec.apply(reply_ok());
    let _ = exec.apply(ExecEvent::Enqueue {
        cell_id: "c1".to_owned(),
    });
    let _ = exec.apply(ExecEvent::Sent {
        cell_id: "c1".to_owned(),
        msg_id: MsgId::new("m2"),
    });
    let _ = apply_iopub(&mut exec, &mut book, &stream("m2", "stdout", "z"));
    assert_eq!(outputs(&book, "c1"), r#"stdout:"a\nz""#);
}

/// `c1` (`m1`) replied but is not idle yet while `c2` (`m2`) runs, and both
/// hold a waiting clear.
fn two_pending() -> (ExecState, Notebook) {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1", "c2"]);
    let _ = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", "a\n"));
    let _ = apply_iopub(&mut exec, &mut book, &clear("m1", true));
    let _ = exec.apply(reply_ok());
    let _ = exec.apply(ExecEvent::Enqueue {
        cell_id: "c2".to_owned(),
    });
    let _ = exec.apply(ExecEvent::Sent {
        cell_id: "c2".to_owned(),
        msg_id: MsgId::new("m2"),
    });
    let _ = apply_iopub(&mut exec, &mut book, &stream("m2", "stdout", "b\n"));
    let _ = apply_iopub(&mut exec, &mut book, &clear("m2", true));
    assert!(exec.is_clear_pending(&m1()));
    assert!(exec.is_clear_pending(&MsgId::new("m2")));
    (exec, book)
}

#[test]
fn removing_one_cell_keeps_the_other_runs_pending_clear() {
    let (mut exec, _book) = two_pending();
    let _ = exec.apply(ExecEvent::RemoveCell {
        cell_id: "c1".to_owned(),
    });
    assert!(!exec.is_clear_pending(&m1()));
    assert!(exec.is_clear_pending(&MsgId::new("m2")));
}

#[test]
fn ending_one_run_keeps_the_other_runs_pending_clear() {
    let (mut exec, _book) = two_pending();
    let _ = exec.apply(idle());
    assert!(!exec.is_clear_pending(&m1()));
    assert!(exec.is_clear_pending(&MsgId::new("m2")));
}
