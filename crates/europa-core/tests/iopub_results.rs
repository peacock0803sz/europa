mod common;

use common::{
    applied, clear, code, display, error, input, notebook, plain, result, running, shown, shows,
    stream, update,
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

fn kind(output: &Output) -> &'static str {
    match output {
        Output::Stream(_) => "stream",
        Output::DisplayData(_) => "display_data",
        Output::ExecuteResult(_) => "execute_result",
        Output::Error(_) => "error",
        Output::Unknown(_) => "unknown",
    }
}

/// The kind of every output of `cell`, in order.
fn kinds(book: &Notebook, cell: &str) -> Vec<&'static str> {
    code(book, cell).outputs.iter().map(kind).collect()
}

/// The output kind a message is expected to leave behind.
fn expected_kind(msg: &IopubMsg) -> &'static str {
    match msg {
        IopubMsg::Stream { .. } => "stream",
        IopubMsg::DisplayData { .. } => "display_data",
        IopubMsg::ExecuteResult { .. } => "execute_result",
        IopubMsg::Error { .. } => "error",
        other => panic!("not an output message: {other:?}"),
    }
}

/// Every ordering of `items`, so that no order of arrival goes untested.
fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut all = Vec::new();
    for (at, item) in items.iter().enumerate() {
        let mut rest = items.to_vec();
        rest.remove(at);
        for mut tail in permutations(&rest) {
            tail.insert(0, item.clone());
            all.push(tail);
        }
    }
    all
}

#[test]
fn a_result_is_appended_with_its_own_count() {
    let (mut exec, mut book) = fresh();
    let msgs = [stream("m1", "stdout", "a\n"), result("m1", Some(3), "42")];
    deliver(&mut exec, &mut book, &msgs);

    assert_eq!(
        shown(&book, "c1"),
        r#"stdout:"a\n", result[3]:{"text/plain":"42"}"#
    );
    // The cell takes its count from execute_input only.
    assert_eq!(code(&book, "c1").execution_count, None);
}

#[test]
fn a_result_keeps_its_metadata_as_sent() {
    let (mut exec, mut book) = fresh();
    let metadata = JsonMap::from([("isolated".to_owned(), JsonValue::from(true))]);
    let msg = IopubMsg::ExecuteResult {
        parent: m1(),
        data: plain("1"),
        metadata: metadata.clone(),
        execution_count: Some(1),
    };
    deliver(&mut exec, &mut book, &[msg]);

    let Output::ExecuteResult(output) = &code(&book, "c1").outputs[0] else {
        panic!("execute result output");
    };
    assert_eq!(output.metadata, metadata);
}

#[test]
fn an_error_is_appended_as_sent_with_its_escape_sequences() {
    let (mut exec, mut book) = fresh();
    let line = "\u{1b}[0;31mZeroDivisionError\u{1b}[0m: division by zero";
    let msg = error("m1", "ZeroDivisionError", "division by zero", &[line]);
    deliver(&mut exec, &mut book, &[msg]);

    let outputs = &code(&book, "c1").outputs;
    assert_eq!(outputs.len(), 1);
    let Output::Error(output) = &outputs[0] else {
        panic!("error output");
    };
    assert_eq!(output.ename, "ZeroDivisionError");
    assert_eq!(output.evalue, "division by zero");
    assert_eq!(output.traceback, [line]);
}

#[test]
fn a_result_never_takes_part_in_display_updates() {
    let (mut exec, mut book) = fresh();
    let msgs = [display("m1", Some("p"), "0%"), result("m1", Some(1), "1")];
    deliver(&mut exec, &mut book, &msgs);
    let result_line = r#"result[1]:{"text/plain":"1"}"#;
    assert_eq!(
        shown(&book, "c1"),
        format!("{}, {result_line}", shows("0%"))
    );

    deliver(&mut exec, &mut book, &[update("m1", "p", "2%")]);
    assert_eq!(
        shown(&book, "c1"),
        format!("{}, {result_line}", shows("2%"))
    );
}

#[test]
fn outputs_of_every_kind_keep_the_order_they_arrived_in() {
    let (mut exec, mut book) = fresh();
    let msgs = [
        stream("m1", "stdout", "a\n"),
        display("m1", None, "d1"),
        stream("m1", "stdout", "b\n"),
        stream("m1", "stderr", "w\n"),
        result("m1", Some(1), "r"),
        stream("m1", "stdout", "c\n"),
        error("m1", "E", "v", &[]),
        display("m1", None, "d2"),
        stream("m1", "stdout", "e\n"),
        stream("m1", "stdout", "f\n"),
    ];
    deliver(&mut exec, &mut book, &msgs);

    assert_eq!(
        kinds(&book, "c1"),
        [
            "stream",
            "display_data",
            "stream",
            "stream",
            "execute_result",
            "stream",
            "error",
            "display_data",
            "stream",
        ]
    );
    let Output::Stream(last) = code(&book, "c1").outputs.last().expect("outputs") else {
        panic!("stream output");
    };
    assert_eq!(last.text.text(), "e\nf\n");
}

// The streams differ in name, so none of them merges with a neighbour and the
// kinds must come out exactly as they went in.
#[test]
fn any_arrival_order_leaves_the_kinds_in_that_order() {
    let msgs = [
        stream("m1", "stdout", "a\n"),
        display("m1", None, "d"),
        result("m1", Some(1), "r"),
        error("m1", "E", "v", &[]),
        stream("m1", "stderr", "w\n"),
    ];
    let orders = permutations(&msgs);
    assert_eq!(orders.len(), 120);

    for order in orders {
        let (mut exec, mut book) = fresh();
        deliver(&mut exec, &mut book, &order);
        let expected: Vec<_> = order.iter().map(expected_kind).collect();
        assert_eq!(kinds(&book, "c1"), expected, "for {order:?}");
    }
}

#[test]
fn a_result_or_an_error_fires_a_pending_clear() {
    for (msg, expected) in [
        (result("m1", Some(1), "x"), "execute_result"),
        (error("m1", "E", "v", &[]), "error"),
    ] {
        let (mut exec, mut book) = fresh();
        let before = [stream("m1", "stdout", "a\n"), clear("m1", true)];
        deliver(&mut exec, &mut book, &before);
        assert!(exec.is_clear_pending(&m1()));

        deliver(&mut exec, &mut book, &[msg]);

        assert_eq!(kinds(&book, "c1"), [expected]);
        assert!(!exec.is_clear_pending(&m1()));
    }
}

/// A notebook whose `c1` has this source and execution count.
fn counted(source: &str, count: &str) -> Notebook {
    let text = format!(
        r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{}},"cells":[{{"cell_type":"code","id":"c1","metadata":{{}},"source":{source:?},"outputs":[],"execution_count":{count}}}]}}"#
    );
    europa_core::parse(text.as_bytes()).expect("fixture notebook loads")
}

#[test]
fn execute_input_replaces_the_execution_count() {
    let mut book = counted("", "3");
    let mut exec = running("c1", "m1");

    let outcome = apply_iopub(&mut exec, &mut book, &input("m1", 7));

    assert_eq!(code(&book, "c1").execution_count, Some(7));
    assert_eq!(outcome, applied("c1", &["c1"]));
    assert!(exec.is_dirty());
}

#[test]
fn execute_input_fills_an_empty_execution_count() {
    let mut book = counted("", "null");
    let mut exec = running("c1", "m1");

    deliver(&mut exec, &mut book, &[input("m1", 1)]);

    assert_eq!(code(&book, "c1").execution_count, Some(1));
}

#[test]
fn execute_input_with_the_same_count_changes_nothing() {
    let mut book = counted("", "null");
    let mut exec = running("c1", "m1");
    deliver(&mut exec, &mut book, &[input("m1", 1)]);
    let _ = exec.apply(ExecEvent::Saved);
    let (exec_before, book_before) = (exec.clone(), book.clone());

    let outcome = apply_iopub(&mut exec, &mut book, &input("m1", 1));

    assert_eq!(outcome, applied("c1", &[]));
    assert!(!exec.is_dirty());
    assert_eq!(exec, exec_before);
    assert_eq!(book, book_before);
}

#[test]
fn a_result_count_that_differs_does_not_move_the_cell_count() {
    let (mut exec, mut book) = fresh();
    deliver(
        &mut exec,
        &mut book,
        &[input("m1", 2), result("m1", Some(5), "x")],
    );

    assert_eq!(code(&book, "c1").execution_count, Some(2));
    assert_eq!(shown(&book, "c1"), r#"result[5]:{"text/plain":"x"}"#);
}

#[test]
fn a_saved_execution_count_survives_a_reload_and_the_source_stays() {
    let mut book = counted("print(1)\n", "null");
    let mut exec = running("c1", "m1");
    deliver(&mut exec, &mut book, &[input("m1", 9)]);

    let reread = europa_core::parse(europa_core::serialize(&book).as_bytes()).expect("loads");

    assert_eq!(code(&reread, "c1").execution_count, Some(9));
    assert_eq!(code(&reread, "c1").source.text(), "print(1)\n");
}

// Only an output makes a waiting clear fire, and a count is not one.
#[test]
fn execute_input_neither_fires_nor_ends_a_pending_clear() {
    let (mut exec, mut book) = fresh();
    let msgs = [stream("m1", "stdout", "a\n"), clear("m1", true)];
    deliver(&mut exec, &mut book, &msgs);

    deliver(&mut exec, &mut book, &[input("m1", 3)]);

    assert_eq!(shown(&book, "c1"), r#"stdout:"a\n""#);
    assert!(exec.is_clear_pending(&m1()));
    assert_eq!(code(&book, "c1").execution_count, Some(3));
}
