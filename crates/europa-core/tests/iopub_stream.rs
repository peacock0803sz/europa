mod common;

use common::{notebook, outputs, running, stream};
use europa_core::{apply_iopub, ExecState, Notebook};

/// Deliver `chunks` as stdout of `m1` to a fresh notebook and return the outputs.
fn deliver(chunks: &[&str]) -> String {
    let (mut exec, mut book) = fresh();
    for chunk in chunks {
        let applied = apply_iopub(&mut exec, &mut book, &stream("m1", "stdout", chunk));
        assert_eq!(applied.as_deref(), Some("c1"));
    }
    outputs(&book, "c1")
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
            let whole = deliver(&[&text]);
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
                    deliver(&parts),
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
