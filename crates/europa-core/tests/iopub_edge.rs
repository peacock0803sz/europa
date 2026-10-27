mod common;

use common::{clear, notebook, outputs, running, stream};
use europa_core::{apply_iopub, MsgId};

#[test]
fn unknown_parent_changes_nothing() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let (exec_before, book_before) = (exec.clone(), book.clone());
    for msg in [stream("other", "stdout", "x"), clear("other", false)] {
        assert_eq!(apply_iopub(&mut exec, &mut book, &msg), None);
    }
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
    assert_eq!(exec.cell_for(&MsgId::new("other")), None);
}

#[test]
fn missing_code_cell_changes_nothing() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c2"]);
    let (exec_before, book_before) = (exec.clone(), book.clone());
    for msg in [stream("m1", "stdout", "x"), clear("m1", false)] {
        assert_eq!(apply_iopub(&mut exec, &mut book, &msg), None);
    }
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
}

#[test]
fn non_empty_stream_marks_dirty() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", "x"));
    assert!(exec.is_dirty());
}

#[test]
fn empty_stream_adds_nothing_and_stays_clean() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let applied = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", ""));
    assert_eq!(applied.as_deref(), Some("c1"));
    assert_eq!(outputs(&book, "c1"), "");
    assert!(!exec.is_dirty());
}
