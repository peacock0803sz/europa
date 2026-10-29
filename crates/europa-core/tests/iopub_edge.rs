mod common;

use common::{
    applied, clear, code, display, error, finish, input, notebook, outputs, result, running, shown,
    start, stream, update,
};
use europa_core::{apply_iopub, Cell, ExecEvent, MsgId, Output};

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

#[test]
fn non_empty_stream_lists_its_cell_as_changed() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let outcome = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", "a"));
    assert_eq!(outcome, applied("c1", &["c1"]));
    let appended = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", "b"));
    assert_eq!(appended, applied("c1", &["c1"]));
    assert_eq!(outputs(&book, "c1"), r#"stdout:"ab""#);
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

#[test]
fn unknown_parent_leaves_displays_alone() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let (exec_before, book_before) = (exec.clone(), book.clone());
    for msg in [display("other", Some("p"), "x"), update("other", "p", "x")] {
        assert_eq!(apply_iopub(&mut exec, &mut book, &msg), None);
    }
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
}

#[test]
fn missing_code_cell_leaves_displays_alone() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c2"]);
    let (exec_before, book_before) = (exec.clone(), book.clone());
    for msg in [display("m1", Some("p"), "x"), update("m1", "p", "x")] {
        assert_eq!(apply_iopub(&mut exec, &mut book, &msg), None);
    }
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
}

// The run is still known, but its cell was dropped from the notebook without
// telling the execution state. Replacing in the other cells anyway would
// change a notebook that the message cannot be tied to.
#[test]
fn a_missing_parent_cell_replaces_nothing_in_other_cells() {
    let mut exec = running("c2", "m2");
    let mut book = notebook(&["c1", "c2"]);
    let _ = apply_iopub(&mut exec, &mut book, &display("m2", Some("p"), "0%"));
    finish(&mut exec, "m2");
    start(&mut exec, "c1", "m1");
    book.cells.retain(|cell| cell.id() != "c1");
    let (exec_before, book_before) = (exec.clone(), book.clone());

    for msg in [display("m1", Some("p"), "x"), update("m1", "p", "x")] {
        assert_eq!(apply_iopub(&mut exec, &mut book, &msg), None);
    }

    assert_eq!(shown(&book, "c2"), r#"display:{"text/plain":"0%"}"#);
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
}

#[test]
fn an_update_after_the_run_ended_changes_nothing() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1", "c2"]);
    let _ = apply_iopub(&mut exec, &mut book, &display("m1", Some("p"), "0%"));
    finish(&mut exec, "m1");
    let (exec_before, book_before) = (exec.clone(), book.clone());

    let late = apply_iopub(&mut exec, &mut book, &update("m1", "p", "1%"));

    assert_eq!(late, None);
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
}

#[test]
fn a_display_marks_the_notebook_dirty() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    assert!(!exec.is_dirty());
    let _ = apply_iopub(&mut exec, &mut book, &display("m1", None, "x"));
    assert!(exec.is_dirty());
}

#[test]
fn an_update_without_a_bound_output_keeps_the_notebook_clean() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let _ = apply_iopub(&mut exec, &mut book, &update("m1", "q", "x"));
    assert!(!exec.is_dirty());
}

// Comparing old and new content would cost a walk over every bundle, and a
// bound output that was rewritten with the same content is no loss.
#[test]
fn an_update_with_the_same_content_still_marks_the_notebook_dirty() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let _ = apply_iopub(&mut exec, &mut book, &display("m1", Some("p"), "0%"));
    let _ = exec.apply(ExecEvent::Saved);
    assert!(!exec.is_dirty());

    let same = apply_iopub(&mut exec, &mut book, &update("m1", "p", "0%"));

    assert!(exec.is_dirty());
    assert_eq!(same, applied("c1", &["c1"]));
}

#[test]
fn a_cell_is_listed_once_however_many_of_its_outputs_are_replaced() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1", "c2"]);
    let first = apply_iopub(&mut exec, &mut book, &display("m1", Some("p"), "0%"));
    assert_eq!(first, applied("c1", &["c1"]));
    let second = apply_iopub(&mut exec, &mut book, &display("m1", Some("p"), "1%"));
    assert_eq!(second, applied("c1", &["c1"]));
    let update_both = apply_iopub(&mut exec, &mut book, &update("m1", "p", "2%"));
    assert_eq!(update_both, applied("c1", &["c1"]));

    finish(&mut exec, "m1");
    start(&mut exec, "c2", "m2");
    let across = apply_iopub(&mut exec, &mut book, &display("m2", Some("p"), "3%"));
    assert_eq!(across, applied("c2", &["c1", "c2"]));
    let everywhere = apply_iopub(&mut exec, &mut book, &update("m2", "p", "z"));
    assert_eq!(everywhere, applied("c2", &["c1", "c2"]));
}

#[test]
fn a_replaced_display_is_saved_without_its_id_and_split_into_lines() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let shown_with = display("m1", Some("display-id-zz"), "a\nb");
    let _ = apply_iopub(&mut exec, &mut book, &shown_with);
    let saved = europa_core::serialize(&book);
    assert!(!saved.contains("display-id-zz"), "{saved}");
    assert!(saved.contains(r#""a\n","#), "{saved}");
    assert!(saved.contains(r#""b""#), "{saved}");
    assert!(code(&book, "c1").outputs[0].is_new());

    let _ = apply_iopub(&mut exec, &mut book, &update("m1", "display-id-zz", "c\nd"));
    let saved = europa_core::serialize(&book);
    assert!(!saved.contains("display-id-zz"), "{saved}");
    assert!(saved.contains(r#""c\n","#), "{saved}");
    assert!(saved.contains(r#""d""#), "{saved}");
    assert!(!saved.contains(r#""a\n","#), "{saved}");
}

const LOADED_DISPLAY: &str = r#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","id":"c1","metadata":{},"source":"","outputs":[{"output_type":"display_data","data":{"text/plain":"keep"},"metadata":{}}],"execution_count":null}]}"#;

#[test]
fn a_loaded_display_is_written_back_as_it_was_beside_a_new_one() {
    let mut book = europa_core::parse(LOADED_DISPLAY.as_bytes()).expect("loads");
    let mut exec = running("c1", "m1");
    let _ = apply_iopub(&mut exec, &mut book, &display("m1", None, "new"));
    let saved = europa_core::serialize(&book);
    assert!(saved.contains(r#""text/plain": "keep""#), "{saved}");
}

// Ids are never saved, so what a notebook holds after a reload cannot be
// replaced by a message of a run that started before it.
#[test]
fn outputs_read_back_from_a_saved_notebook_are_not_replaced() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let _ = apply_iopub(&mut exec, &mut book, &display("m1", Some("p"), "0%"));
    let mut reread = europa_core::parse(europa_core::serialize(&book).as_bytes()).expect("loads");
    let before = reread.clone();

    let outcome = apply_iopub(&mut exec, &mut reread, &update("m1", "p", "1%"));

    assert_eq!(outcome, applied("c1", &[]));
    assert_eq!(reread, before);
}

#[test]
fn unknown_parent_leaves_results_and_errors_alone() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let (exec_before, book_before) = (exec.clone(), book.clone());
    for msg in [
        result("other", Some(1), "x"),
        error("other", "E", "v", &["t"]),
    ] {
        assert_eq!(apply_iopub(&mut exec, &mut book, &msg), None);
    }
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
}

#[test]
fn missing_code_cell_leaves_results_and_errors_alone() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c2"]);
    let (exec_before, book_before) = (exec.clone(), book.clone());
    for msg in [result("m1", Some(1), "x"), error("m1", "E", "v", &["t"])] {
        assert_eq!(apply_iopub(&mut exec, &mut book, &msg), None);
    }
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
}

#[test]
fn unknown_parent_leaves_the_execution_count_alone() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let (exec_before, book_before) = (exec.clone(), book.clone());
    assert_eq!(apply_iopub(&mut exec, &mut book, &input("other", 3)), None);
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
}

#[test]
fn missing_code_cell_leaves_the_execution_count_alone() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c2"]);
    let (exec_before, book_before) = (exec.clone(), book.clone());
    assert_eq!(apply_iopub(&mut exec, &mut book, &input("m1", 3)), None);
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
}

#[test]
fn a_result_and_an_error_are_saved_as_new_outputs() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    let _ = apply_iopub(&mut exec, &mut book, &result("m1", Some(1), "x\ny"));
    let _ = apply_iopub(&mut exec, &mut book, &error("m1", "E", "v", &["t1", "t2"]));

    let saved = europa_core::serialize(&book);
    assert!(saved.contains(r#""x\n","#), "{saved}");
    assert!(saved.contains(r#""y""#), "{saved}");
    assert!(saved.contains(r#""t1""#), "{saved}");
    assert!(saved.contains(r#""t2""#), "{saved}");
    let outputs = &code(&book, "c1").outputs;
    assert_eq!(outputs.len(), 2);
    assert!(outputs.iter().all(Output::is_new));
}

#[test]
fn a_result_marks_the_notebook_dirty() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    assert!(!exec.is_dirty());

    let outcome = apply_iopub(&mut exec, &mut book, &result("m1", Some(1), "x"));

    assert!(exec.is_dirty());
    assert_eq!(outcome, applied("c1", &["c1"]));
}

#[test]
fn an_error_marks_the_notebook_dirty() {
    let mut exec = running("c1", "m1");
    let mut book = notebook(&["c1"]);
    assert!(!exec.is_dirty());

    let outcome = apply_iopub(&mut exec, &mut book, &error("m1", "E", "v", &[]));

    assert!(exec.is_dirty());
    assert_eq!(outcome, applied("c1", &["c1"]));
}
