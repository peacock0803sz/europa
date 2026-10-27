mod common;

use common::{clear, notebook, outputs, running, stream};
use europa_core::{apply_iopub, Cell, MsgId, Output};

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
    assert_eq!(applied.as_deref(), Some("c1"));
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
