mod common;

use common::{code, notebook, outputs, running, stream};
use europa_core::{apply_iopub, resolve_carriage_returns, ExecState, Notebook, Output};

/// Deliver `chunks` as stdout of `m1` to a fresh notebook and return the outputs.
fn deliver(chunks: &[&str]) -> String {
    outputs(&deliver_to_notebook(chunks), "c1")
}

/// Like [`deliver`], but also return the saved notebook.
fn deliver_and_save(chunks: &[&str]) -> (String, String) {
    let book = deliver_to_notebook(chunks);
    (outputs(&book, "c1"), europa_core::serialize(&book))
}

fn deliver_to_notebook(chunks: &[&str]) -> Notebook {
    let (mut exec, mut book) = fresh();
    for chunk in chunks {
        let applied = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", chunk));
        assert_eq!(applied.map(|applied| applied.cell), Some("c1".to_owned()));
    }
    book
}

fn fresh() -> (ExecState, Notebook) {
    (running("c1", "m1"), notebook(&["c1"]))
}

#[test]
fn overwrites_progress_in_place() {
    assert_eq!(deliver(&["10%", "\r20%", "\r30%"]), r#"stdout:"30%""#);
}

#[test]
fn keeps_finished_line_and_continues_after_it() {
    assert_eq!(
        deliver(&["10%", "\r20%", "\r30%", "\r100%\n"]),
        r#"stdout:"100%\n""#
    );
    assert_eq!(
        deliver(&["10%", "\r20%", "\r30%", "\r100%\n", "next"]),
        r#"stdout:"100%\nnext""#
    );
}

#[test]
fn overwrite_only_touches_the_last_line() {
    assert_eq!(deliver(&["a\nb", "\rc"]), r#"stdout:"a\nc""#);
}

#[test]
fn trailing_carriage_return_waits_for_the_next_chunk() {
    assert_eq!(deliver(&["abc\r", "d"]), r#"stdout:"d""#);
    assert_eq!(deliver(&["abc\r", "\ndef"]), r#"stdout:"abc\ndef""#);
}

#[test]
fn carriage_return_replaces_the_whole_line() {
    assert_eq!(deliver(&["abcdef", "\rXY"]), r#"stdout:"XY""#);
}

#[test]
fn any_split_gives_the_same_result_as_one_chunk() {
    const ALPHABET: [char; 4] = ['a', 'b', '\r', '\n'];
    for len in 1..=6_u32 {
        for code in 0..4_usize.pow(len) {
            let text: String = (0..len)
                .map(|at| ALPHABET[code / 4_usize.pow(at) % 4])
                .collect();
            let whole = deliver_and_save(&[&text]);
            let chars: Vec<char> = text.chars().collect();
            for mask in 1..(1_u32 << (len - 1)) {
                let mut chunks = Vec::new();
                let mut start = 0;
                for at in 1..chars.len() {
                    if mask >> (at - 1) & 1 == 1 {
                        chunks.push(chars[start..at].iter().collect::<String>());
                        start = at;
                    }
                }
                chunks.push(chars[start..].iter().collect::<String>());
                let parts: Vec<&str> = chunks.iter().map(String::as_str).collect();
                assert_eq!(
                    deliver_and_save(&parts),
                    whole,
                    "text {text:?} split as {parts:?} differs from one chunk"
                );
            }
        }
    }
}

#[test]
fn hundred_progress_updates_leave_one_output() {
    let mut chunks = vec!["0%".to_owned()];
    chunks.extend((1..=100).map(|percent| format!("\r{percent}%")));
    let parts: Vec<&str> = chunks.iter().map(String::as_str).collect();
    assert_eq!(deliver(&parts), r#"stdout:"100%""#);
}

fn deliver_all(messages: &[(&str, &str)]) -> String {
    let (mut exec, mut book) = fresh();
    for (name, text) in messages {
        let _ = apply_iopub(&mut exec, &mut book, &stream("m1", name, text));
    }
    outputs(&book, "c1")
}

#[test]
fn same_name_chunks_merge() {
    assert_eq!(
        deliver_all(&[("stdout", "a\n"), ("stdout", "b\n")]),
        r#"stdout:"a\nb\n""#
    );
}

#[test]
fn name_switch_starts_a_new_output() {
    assert_eq!(
        deliver_all(&[("stdout", "a\n"), ("stderr", "w\n"), ("stdout", "b\n")]),
        r#"stdout:"a\n", stderr:"w\n", stdout:"b\n""#
    );
}

#[test]
fn carriage_return_in_stderr_leaves_stdout_alone() {
    assert_eq!(
        deliver_all(&[("stdout", "a\n"), ("stderr", "x\ry")]),
        r#"stdout:"a\n", stderr:"y""#
    );
}

#[test]
fn alternating_names_give_one_output_per_switch() {
    let messages: Vec<(&str, &str)> = (0..100)
        .map(|at| (if at % 2 == 0 { "stdout" } else { "stderr" }, "x\n"))
        .collect();
    let result = deliver_all(&messages);
    assert_eq!(result.split(", ").count(), 100);
    assert!(result.starts_with("stdout:"));
    assert!(result.contains(r#"stdout:"x\n", stderr:"x\n", stdout:"#));
}

#[test]
fn custom_stream_names_merge_like_any_other() {
    assert_eq!(
        deliver_all(&[("custom", "a"), ("custom", "b")]),
        r#"custom:"ab""#
    );
}

const LOADED_BODIES: [&str; 9] = [
    "10%\r20%\r30%",
    "abcdef\rXY",
    "abc\r",
    "a\r\r\nb",
    "\u{1b}[31m10%\r20%",
    "a\nb\rc",
    "x\r\ry\r\r",
    "plain\n",
    "",
];

#[test]
fn a_loaded_body_resolves_carriage_returns_as_received_text_does() {
    let resolved = [
        "30%", "XY", "abc\r", "a\nb", "20%", "a\nc", "y\r", "plain\n", "",
    ];
    for (body, resolved) in LOADED_BODIES.into_iter().zip(resolved) {
        assert_eq!(resolve_carriage_returns(body), resolved, "{body:?}");
    }
}

#[test]
fn resolving_gives_the_text_that_one_message_leaves_in_the_notebook() {
    for body in LOADED_BODIES.into_iter().filter(|body| !body.is_empty()) {
        let book = deliver_to_notebook(&[body]);
        let Output::Stream(received) = &code(&book, "c1").outputs[0] else {
            panic!("no stream output for {body:?}");
        };
        assert_eq!(
            received.text.text(),
            resolve_carriage_returns(body),
            "{body:?}"
        );
    }
}

#[test]
fn stream_after_a_non_stream_output_starts_a_new_output() {
    let text = r#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","id":"c1","metadata":{},"source":"","outputs":[{"output_type":"display_data","data":{},"metadata":{}}],"execution_count":null}]}"#;
    let mut book = europa_core::parse(text.as_bytes()).expect("loads");
    let mut exec = running("c1", "m1");
    let _ = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", "a"));
    assert_eq!(outputs(&book, "c1"), r#"<display_data>, stdout:"a""#);
}
