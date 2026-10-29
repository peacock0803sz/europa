mod common;

use common::{clear, notebook, outputs, running, stream};
use europa_core::{apply_iopub, Cell, IopubApplied, MsgId, Output};

#[test]
fn unknown_parent_changes_nothing() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let (exec_before, book_before) = (exec.clone(), book.clone());
    for msg in [
        stream("other", "stdout", "x"),
        clear("other", false),
        clear("other", true),
    ] {
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
    for msg in [
        stream("m1", "stdout", "x"),
        clear("m1", false),
        clear("m1", true),
    ] {
        assert_eq!(apply_iopub(&mut exec, &mut book, &msg), None);
    }
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
}

#[test]
fn non_empty_stream_marks_dirty() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let _ = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", "x"));
    assert!(exec.is_dirty());
}

#[test]
fn empty_stream_adds_nothing_and_stays_clean() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let applied = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", ""));
    assert_eq!(applied.map(|applied| applied.cell), Some("c1".to_owned()));
    assert_eq!(outputs(&book, "c1"), "");
    assert!(!exec.is_dirty());
}

const LOADED: &str = r#"{
 "cells": [
  {
   "cell_type": "code",
   "execution_count": null,
   "id": "c1",
   "metadata": {},
   "outputs": [
    {
     "name": "stdout",
     "output_type": "stream",
     "text": [
      "p\r",
      "q\n",
      "50%"
     ],
     "x-extra": 1
    }
   ],
   "source": ""
  }
 ],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 5
}
"#;

#[test]
fn loaded_stream_is_written_back_untouched() {
    let book = europa_core::parse(LOADED.as_bytes()).expect("loads");
    assert_eq!(europa_core::serialize(&book), LOADED);
}

#[test]
fn appending_to_a_loaded_stream_rebuilds_it_as_new() {
    let mut book = europa_core::parse(LOADED.as_bytes()).expect("loads");
    let mut exec = running("c1", "m1");
    let applied = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", "\r60%"));
    assert_eq!(applied.map(|applied| applied.cell), Some("c1".to_owned()));
    assert_eq!(outputs(&book, "c1"), r#"stdout:"p\rq\n60%""#);

    let Cell::Code(code) = &book.cells[0] else {
        panic!("code cell");
    };
    assert_eq!(code.outputs.len(), 1);
    assert!(code.outputs[0].is_new());
    let Output::Stream(merged) = &code.outputs[0] else {
        panic!("stream output");
    };
    assert!(merged.extra.contains_key("x-extra"), "unknown field kept");

    let saved = europa_core::serialize(&book);
    assert!(saved.contains(r#""60%""#));
    assert!(!saved.contains(r#""50%""#));
}

const LOADED_SINGLE: &str = r#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","id":"c1","metadata":{},"source":"","outputs":[{"output_type":"stream","name":"stdout","text":"a\r\nb","x-extra":1}],"execution_count":null}]}"#;

// A loaded output held as one string could be edited in place, but that would
// keep it loaded and write the CRLF text back unsplit.
#[test]
fn appending_to_a_loaded_stream_held_as_a_string_rebuilds_it_as_new() {
    let mut book = europa_core::parse(LOADED_SINGLE.as_bytes()).expect("loads");
    let mut exec = running("c1", "m1");
    let _ = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", "\rc"));
    assert_eq!(outputs(&book, "c1"), r#"stdout:"a\nc""#);

    let Cell::Code(code) = &book.cells[0] else {
        panic!("code cell");
    };
    assert!(code.outputs[0].is_new());
    let Output::Stream(merged) = &code.outputs[0] else {
        panic!("stream output");
    };
    assert!(merged.extra.contains_key("x-extra"), "unknown field kept");

    let saved = europa_core::serialize(&book);
    let lines = "     \"text\": [\n      \"a\\n\",\n      \"c\"\n     ],";
    assert!(saved.contains(lines), "{saved}");
}

#[test]
fn appending_to_a_new_stream_held_as_an_array_rebuilds_it_as_a_string() {
    let mut book = notebook(&["c1"]);
    let Cell::Code(code) = &mut book.cells[0] else {
        panic!("code cell");
    };
    code.outputs.push(Output::new_stream("stdout", "a\n"));
    let Output::Stream(existing) = &mut code.outputs[0] else {
        panic!("stream output");
    };
    existing.text = serde_json::from_value(serde_json::json!(["a\n"])).expect("array text");

    let mut exec = running("c1", "m1");
    let _ = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", "b"));
    assert_eq!(outputs(&book, "c1"), r#"stdout:"a\nb""#);

    let Cell::Code(code) = &book.cells[0] else {
        panic!("code cell");
    };
    assert_eq!(code.outputs.len(), 1);
    assert!(code.outputs[0].is_new());
    let Output::Stream(merged) = &code.outputs[0] else {
        panic!("stream output");
    };
    assert!(serde_json::to_value(&merged.text)
        .expect("serializes")
        .is_string());
}

const WITH_OUTPUT: &str = r#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","id":"c1","metadata":{},"source":"","outputs":[{"output_type":"stream","name":"stdout","text":"old\n"}],"execution_count":null}]}"#;

#[test]
fn waiting_clear_keeps_the_notebook_clean() {
    let mut book = europa_core::parse(WITH_OUTPUT.as_bytes()).expect("loads");
    let mut exec = running("c1", "m1");
    let _ = apply_iopub(&mut exec, &mut book, &clear("m1", true));
    assert!(!exec.is_dirty());
}

#[test]
fn immediate_clear_of_empty_cell_keeps_the_notebook_clean() {
    let mut book = notebook(&["c1"]);
    let mut exec = running("c1", "m1");
    let _ = apply_iopub(&mut exec, &mut book, &clear("m1", false));
    assert!(!exec.is_dirty());
}

#[test]
fn immediate_clear_of_existing_output_marks_dirty() {
    let mut book = europa_core::parse(WITH_OUTPUT.as_bytes()).expect("loads");
    let mut exec = running("c1", "m1");
    let _ = apply_iopub(&mut exec, &mut book, &clear("m1", false));
    assert_eq!(outputs(&book, "c1"), "");
    assert!(exec.is_dirty());
}

/// What `apply_iopub` returns for a message of `cell` that changed `changed`.
fn applied(cell: &str, changed: &[&str]) -> Option<IopubApplied> {
    Some(IopubApplied {
        cell: cell.to_owned(),
        changed: changed.iter().map(|id| (*id).to_owned()).collect(),
    })
}

#[test]
fn non_empty_stream_lists_its_cell_as_changed() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let result = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", "a"));
    assert_eq!(result, applied("c1", &["c1"]));
}

#[test]
fn empty_stream_and_waiting_clear_change_no_cell() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let empty = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", ""));
    assert_eq!(empty, applied("c1", &[]));
    let waiting = apply_iopub(&mut exec, &mut book, &clear("m1", true));
    assert_eq!(waiting, applied("c1", &[]));
}

#[test]
fn immediate_clear_lists_its_cell_only_when_it_removed_an_output() {
    let mut book = europa_core::parse(WITH_OUTPUT.as_bytes()).expect("loads");
    let mut exec = running("c1", "m1");
    let removed = apply_iopub(&mut exec, &mut book, &clear("m1", false));
    assert_eq!(removed, applied("c1", &["c1"]));
    let nothing_left = apply_iopub(&mut exec, &mut book, &clear("m1", false));
    assert_eq!(nothing_left, applied("c1", &[]));
}
