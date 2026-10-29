mod common;

use common::{
    applied, clear, code, code_mut, describe, display, finish, notebook, plain, running, shown,
    shows, start, stream, update,
};
use europa_core::{
    apply_iopub, ExecEvent, ExecState, IopubMsg, JsonMap, JsonValue, MsgId, Notebook, Output,
};

/// Two empty cells with `c1` running as `m1`.
fn fresh() -> (ExecState, Notebook) {
    (running("c1", "m1"), notebook(&["c1", "c2"]))
}

/// Apply the messages in order, ignoring what `apply_iopub` returns.
fn deliver(exec: &mut ExecState, book: &mut Notebook, msgs: &[IopubMsg]) {
    for msg in msgs {
        let _ = apply_iopub(exec, book, msg);
    }
}

fn m1() -> MsgId {
    MsgId::new("m1")
}

#[test]
fn display_data_joins_the_outputs_without_merging_into_streams() {
    let (mut exec, mut book) = fresh();
    let msgs = [
        stream("m1", "stdout", "a\n"),
        display("m1", None, "plot"),
        stream("m1", "stdout", "b\n"),
    ];
    deliver(&mut exec, &mut book, &msgs);
    let expected = format!(r#"stdout:"a\n", {}, stdout:"b\n""#, shows("plot"));
    assert_eq!(shown(&book, "c1"), expected);
}

#[test]
fn display_data_with_an_empty_bundle_still_adds_an_output() {
    let (mut exec, mut book) = fresh();
    let empty = IopubMsg::DisplayData {
        parent: m1(),
        data: JsonMap::new(),
        metadata: JsonMap::new(),
        display_id: None,
    };
    deliver(&mut exec, &mut book, &[empty]);
    assert_eq!(shown(&book, "c1"), "display:{}");
}

#[test]
fn an_empty_display_id_binds_nothing() {
    let (mut exec, mut book) = fresh();
    let msgs = [display("m1", Some(""), "x"), update("m1", "", "y")];
    deliver(&mut exec, &mut book, &msgs);
    assert_eq!(shown(&book, "c1"), shows("x"));
}

#[test]
fn an_update_for_an_id_nothing_was_shown_with_changes_nothing() {
    let (mut exec, mut book) = fresh();
    let msgs = [display("m1", None, "x"), update("m1", "p", "y")];
    deliver(&mut exec, &mut book, &msgs);
    assert_eq!(shown(&book, "c1"), shows("x"));
}

// An update with an empty id passes whether or not the empty string is bound,
// so only a second display with that id can tell the two apart.
#[test]
fn displays_with_an_empty_id_do_not_replace_each_other() {
    let (mut exec, mut book) = fresh();
    let msgs = [display("m1", Some(""), "x"), display("m1", Some(""), "y")];
    deliver(&mut exec, &mut book, &msgs);
    let expected = format!("{}, {}", shows("x"), shows("y"));
    assert_eq!(shown(&book, "c1"), expected);
}

#[test]
fn an_update_replaces_the_bound_output_in_place() {
    let (mut exec, mut book) = fresh();
    deliver(&mut exec, &mut book, &[display("m1", Some("p"), "0%")]);
    assert_eq!(shown(&book, "c1"), shows("0%"));

    let result = apply_iopub(&mut exec, &mut book, &update("m1", "p", "50%"));
    assert_eq!(shown(&book, "c1"), shows("50%"));
    assert_eq!(result, applied("c1", &["c1"]));
}

#[test]
fn an_update_leaves_the_streams_around_the_output_alone() {
    let (mut exec, mut book) = fresh();
    let msgs = [
        stream("m1", "stdout", "before\n"),
        display("m1", Some("p"), "0%"),
        stream("m1", "stdout", "after\n"),
    ];
    deliver(&mut exec, &mut book, &msgs);
    let before = code(&book, "c1").outputs.clone();

    deliver(&mut exec, &mut book, &[update("m1", "p", "50%")]);

    let after = &code(&book, "c1").outputs;
    assert_eq!(after.len(), 3);
    assert_eq!(after[0], before[0]);
    assert_eq!(after[2], before[2]);
    assert_eq!(describe(&after[1]), shows("50%"));
}

#[test]
fn an_update_from_another_cell_replaces_the_output_where_it_was_shown() {
    let (mut exec, mut book) = fresh();
    deliver(&mut exec, &mut book, &[display("m1", Some("p"), "0%")]);
    finish(&mut exec, "m1");
    start(&mut exec, "c2", "m2");

    let result = apply_iopub(&mut exec, &mut book, &update("m2", "p", "50%"));

    assert_eq!(shown(&book, "c1"), shows("50%"));
    assert_eq!(shown(&book, "c2"), "");
    assert_eq!(result, applied("c2", &["c1"]));
}

#[test]
fn an_update_reaches_every_output_bound_to_the_id() {
    let (mut exec, mut book) = fresh();
    deliver(&mut exec, &mut book, &[display("m1", Some("p"), "a")]);
    finish(&mut exec, "m1");
    start(&mut exec, "c2", "m2");
    let msgs = [display("m2", Some("p"), "b"), update("m2", "p", "c")];
    deliver(&mut exec, &mut book, &msgs);

    assert_eq!(shown(&book, "c1"), shows("c"));
    assert_eq!(shown(&book, "c2"), shows("c"));
}

#[test]
fn an_update_for_an_unbound_id_changes_nothing() {
    let (mut exec, mut book) = fresh();
    deliver(&mut exec, &mut book, &[display("m1", Some("p"), "0%")]);
    let (exec_before, book_before) = (exec.clone(), book.clone());

    let result = apply_iopub(&mut exec, &mut book, &update("m1", "q", "x"));

    assert_eq!(result, applied("c1", &[]));
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
}

#[test]
fn a_hundred_updates_leave_one_output_with_the_last_content() {
    let (mut exec, mut book) = fresh();
    deliver(&mut exec, &mut book, &[display("m1", Some("p"), "0%")]);
    for percent in 1..=100 {
        let msg = update("m1", "p", &format!("{percent}%"));
        deliver(&mut exec, &mut book, &[msg]);
    }
    assert_eq!(shown(&book, "c1"), shows("100%"));
}

#[test]
fn an_update_changes_nothing_but_the_bound_output() {
    let (mut exec, mut book) = fresh();
    let msgs = [
        stream("m1", "stdout", "s\n"),
        display("m1", None, "keep"),
        display("m1", Some("p"), "0%"),
    ];
    deliver(&mut exec, &mut book, &msgs);
    finish(&mut exec, "m1");
    start(&mut exec, "c2", "m2");
    deliver(&mut exec, &mut book, &[stream("m2", "stdout", "t\n")]);
    let before = book.clone();

    deliver(&mut exec, &mut book, &[update("m2", "p", "1%")]);

    let (kept, replaced) = code(&book, "c1").outputs.split_at(2);
    assert_eq!(kept, &code(&before, "c1").outputs[..2]);
    assert_eq!(describe(&replaced[0]), shows("1%"));
    assert_eq!(code(&book, "c2").outputs, code(&before, "c2").outputs);
}

#[test]
fn an_update_replaces_the_whole_bundle_and_the_metadata() {
    let (mut exec, mut book) = fresh();
    let first = IopubMsg::DisplayData {
        parent: m1(),
        data: JsonMap::from([
            ("text/plain".to_owned(), JsonValue::from("0%")),
            ("text/html".to_owned(), JsonValue::from("<b>0%</b>")),
        ]),
        metadata: JsonMap::new(),
        display_id: Some("p".to_owned()),
    };
    let metadata = JsonMap::from([("done".to_owned(), JsonValue::from(true))]);
    let second = IopubMsg::UpdateDisplayData {
        parent: m1(),
        data: plain("1%"),
        metadata: metadata.clone(),
        display_id: "p".to_owned(),
    };
    deliver(&mut exec, &mut book, &[first, second]);

    let Output::DisplayData(output) = &code(&book, "c1").outputs[0] else {
        panic!("display output");
    };
    assert_eq!(output.data, plain("1%"));
    assert_eq!(output.metadata, metadata);
}

#[test]
fn a_display_with_a_shown_id_replaces_it_in_other_cells_and_adds_its_own() {
    let (mut exec, mut book) = fresh();
    deliver(&mut exec, &mut book, &[display("m1", Some("p"), "0%")]);
    finish(&mut exec, "m1");
    start(&mut exec, "c2", "m2");

    let result = apply_iopub(&mut exec, &mut book, &display("m2", Some("p"), "50%"));

    assert_eq!(shown(&book, "c1"), shows("50%"));
    assert_eq!(shown(&book, "c2"), shows("50%"));
    assert_eq!(result, applied("c2", &["c1", "c2"]));
}

#[test]
fn a_repeated_display_replaces_the_earlier_one_and_both_follow_updates() {
    let (mut exec, mut book) = fresh();
    let msgs = [
        display("m1", Some("p"), "0%"),
        display("m1", Some("p"), "1%"),
    ];
    deliver(&mut exec, &mut book, &msgs);
    assert_eq!(
        shown(&book, "c1"),
        format!("{}, {}", shows("1%"), shows("1%"))
    );

    deliver(&mut exec, &mut book, &[update("m1", "p", "2%")]);
    assert_eq!(
        shown(&book, "c1"),
        format!("{}, {}", shows("2%"), shows("2%"))
    );
}

#[test]
fn clearing_the_output_ends_its_binding() {
    let (mut exec, mut book) = fresh();
    let msgs = [display("m1", Some("p"), "a"), clear("m1", false)];
    deliver(&mut exec, &mut book, &msgs);

    let result = apply_iopub(&mut exec, &mut book, &update("m1", "p", "b"));

    assert_eq!(shown(&book, "c1"), "");
    assert_eq!(result, applied("c1", &[]));
}

#[test]
fn a_pending_clear_that_fires_ends_the_binding() {
    let (mut exec, mut book) = fresh();
    let msgs = [
        display("m1", Some("p"), "a"),
        clear("m1", true),
        stream("m1", "stdout", "d\n"),
    ];
    deliver(&mut exec, &mut book, &msgs);

    let result = apply_iopub(&mut exec, &mut book, &update("m1", "p", "b"));

    assert_eq!(shown(&book, "c1"), r#"stdout:"d\n""#);
    assert_eq!(result, applied("c1", &[]));
}

// A cell's outputs are public, so the caller that runs a cell again may empty
// them without telling this crate.
#[test]
fn emptying_the_outputs_directly_ends_the_binding() {
    let (mut exec, mut book) = fresh();
    deliver(&mut exec, &mut book, &[display("m1", Some("p"), "a")]);
    code_mut(&mut book, "c1").outputs.clear();

    let result = apply_iopub(&mut exec, &mut book, &update("m1", "p", "b"));

    assert_eq!(shown(&book, "c1"), "");
    assert_eq!(result, applied("c1", &[]));
}

#[test]
fn removing_the_cell_ends_its_binding() {
    let (mut exec, mut book) = fresh();
    deliver(&mut exec, &mut book, &[display("m1", Some("p"), "a")]);
    finish(&mut exec, "m1");
    start(&mut exec, "c2", "m2");
    book.cells.retain(|cell| cell.id() != "c1");
    let _ = exec.apply(ExecEvent::RemoveCell {
        cell_id: "c1".to_owned(),
    });

    let result = apply_iopub(&mut exec, &mut book, &update("m2", "p", "b"));

    assert_eq!(shown(&book, "c2"), "");
    assert_eq!(result, applied("c2", &[]));
}

#[test]
fn a_reset_ends_every_binding_made_before_it() {
    let (mut exec, mut book) = fresh();
    deliver(&mut exec, &mut book, &[display("m1", Some("p"), "old")]);
    let _ = exec.apply(ExecEvent::Reset);
    start(&mut exec, "c2", "m2");

    let stale = apply_iopub(&mut exec, &mut book, &update("m2", "p", "x"));
    assert_eq!(shown(&book, "c1"), shows("old"));
    assert_eq!(stale, applied("c2", &[]));

    deliver(&mut exec, &mut book, &[display("m2", Some("p"), "new")]);
    let current = apply_iopub(&mut exec, &mut book, &update("m2", "p", "newer"));
    assert_eq!(shown(&book, "c1"), shows("old"));
    assert_eq!(shown(&book, "c2"), shows("newer"));
    assert_eq!(current, applied("c2", &["c2"]));
}

#[test]
fn a_display_fires_a_pending_clear() {
    let (mut exec, mut book) = fresh();
    let msgs = [
        stream("m1", "stdout", "a\n"),
        clear("m1", true),
        display("m1", None, "x"),
    ];
    deliver(&mut exec, &mut book, &msgs);

    assert_eq!(shown(&book, "c1"), shows("x"));
    assert!(!exec.is_clear_pending(&m1()));
}

#[test]
fn an_update_neither_fires_nor_ends_a_pending_clear() {
    let (mut exec, mut book) = fresh();
    let msgs = [
        display("m1", Some("p"), "0%"),
        clear("m1", true),
        update("m1", "p", "50%"),
    ];
    deliver(&mut exec, &mut book, &msgs);

    assert_eq!(shown(&book, "c1"), shows("50%"));
    assert!(exec.is_clear_pending(&m1()));
}

// The pending clear empties the cell the message belongs to first, so the
// replacement that follows can only reach the other cells.
#[test]
fn a_display_clears_its_own_cell_before_replacing_the_others() {
    let mut exec = running("c2", "m2");
    let mut book = notebook(&["c1", "c2"]);
    deliver(&mut exec, &mut book, &[display("m2", Some("p"), "0%")]);
    finish(&mut exec, "m2");
    start(&mut exec, "c1", "m1");
    let msgs = [stream("m1", "stdout", "a\n"), clear("m1", true)];
    deliver(&mut exec, &mut book, &msgs);

    let result = apply_iopub(&mut exec, &mut book, &display("m1", Some("p"), "x"));

    assert_eq!(shown(&book, "c1"), shows("x"));
    assert_eq!(shown(&book, "c2"), shows("x"));
    assert!(!exec.is_clear_pending(&m1()));
    assert_eq!(result, applied("c1", &["c1", "c2"]));
}

#[test]
fn an_empty_display_bundle_fires_a_pending_clear_too() {
    let (mut exec, mut book) = fresh();
    let empty = IopubMsg::DisplayData {
        parent: m1(),
        data: JsonMap::new(),
        metadata: JsonMap::new(),
        display_id: None,
    };
    let msgs = [stream("m1", "stdout", "a\n"), clear("m1", true), empty];
    deliver(&mut exec, &mut book, &msgs);

    assert_eq!(shown(&book, "c1"), "display:{}");
    assert!(!exec.is_clear_pending(&m1()));
}
