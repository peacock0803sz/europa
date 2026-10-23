//! Outputs built by Europa are written the way nbformat writes them.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use europa_core::{parse, serialize, Cell, Notebook, Output};
use serde_json::Value;

fn fixture(kind: &str, name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden")
        .join(kind)
        .join(name);
    std::fs::read(&path).unwrap_or_else(|_| panic!("missing fixture {kind}/{name}"))
}

fn ch(code: u32) -> char {
    char::from_u32(code).expect("valid scalar value")
}

fn separators() -> Vec<char> {
    [
        0x0A, 0x0D, 0x0B, 0x0C, 0x1C, 0x1D, 0x1E, 0x85, 0x2028, 0x2029,
    ]
    .into_iter()
    .map(ch)
    .collect()
}

fn code_outputs<'a>(notebook: &'a mut Notebook, id: &str) -> &'a mut Vec<Output> {
    notebook
        .cells
        .iter_mut()
        .find_map(|cell| match cell {
            Cell::Code(code) if cell_id_matches(code.extra.get("id"), id) => {
                Some(&mut code.outputs)
            }
            _ => None,
        })
        .expect("code cell with the id")
}

fn cell_id_matches(value: Option<&europa_core::JsonValue>, id: &str) -> bool {
    value.and_then(europa_core::JsonValue::as_str) == Some(id)
}

/// Rebuild every output of the `new-outputs` cell as a new output.
fn new_outputs_from_foreign() -> Vec<Output> {
    let foreign = parse(&fixture("foreign", "new_outputs.ipynb")).expect("foreign parses");
    let raw: Value =
        serde_json::from_slice(&fixture("foreign", "new_outputs.ipynb")).expect("foreign is JSON");
    let Cell::Code(code) = &foreign.cells[0] else {
        panic!("code cell");
    };
    code.outputs
        .iter()
        .enumerate()
        .map(|(idx, output)| match output {
            Output::Stream(stream) => {
                let text = raw
                    .pointer(&format!("/cells/0/outputs/{idx}/text"))
                    .and_then(Value::as_str)
                    .expect("stream text is one string");
                Output::new_stream(stream.name.clone(), text)
            }
            Output::DisplayData(data) => {
                Output::new_display_data(data.data.clone(), data.metadata.clone())
            }
            Output::ExecuteResult(result) => Output::new_execute_result(
                result.data.clone(),
                result.metadata.clone(),
                result.execution_count,
            ),
            Output::Error(error) => Output::new_error(
                error.ename.clone(),
                error.evalue.clone(),
                error.traceback.clone(),
            ),
            Output::Unknown(_) => panic!("no unknown outputs in the fixture"),
        })
        .collect()
}

fn first_difference(expected: &str, actual: &str) -> String {
    let mut expected_lines = expected.split('\n');
    let mut actual_lines = actual.split('\n');
    let mut number = 1;
    loop {
        match (expected_lines.next(), actual_lines.next()) {
            (Some(left), Some(right)) if left == right => number += 1,
            (None, None) => return "no difference".to_owned(),
            (left, right) => {
                return format!(
                    "line {number}: expected {:?}, got {:?}",
                    left.unwrap_or("<end>"),
                    right.unwrap_or("<end>")
                )
            }
        }
    }
}

fn replaced_notebook() -> Notebook {
    let mut notebook = parse(&fixture("jupyter", "new_outputs.ipynb")).expect("jupyter parses");
    *code_outputs(&mut notebook, "new-outputs") = new_outputs_from_foreign();
    notebook
}

#[test]
fn new_outputs_match_what_nbformat_writes() {
    let raw: Value =
        serde_json::from_slice(&fixture("foreign", "new_outputs.ipynb")).expect("foreign is JSON");
    let text = raw
        .pointer("/cells/0/outputs/0/text")
        .and_then(Value::as_str)
        .expect("stdout text");
    let mime = raw
        .pointer("/cells/0/outputs/2/data/text~1plain")
        .and_then(Value::as_str)
        .expect("text/plain");
    for sep in separators() {
        assert!(text.contains(sep), "stdout text has {:?}", sep);
        assert!(mime.contains(sep), "text/plain has {:?}", sep);
    }
    assert!(text.contains("\r\n"));
    let jupyter: Value =
        serde_json::from_slice(&fixture("jupyter", "new_outputs.ipynb")).expect("jupyter is JSON");
    assert_eq!(
        jupyter
            .pointer("/cells/0/outputs/0/text")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(12)
    );

    let saved = serialize(&replaced_notebook());
    let expected = String::from_utf8(fixture("jupyter", "new_outputs.ipynb")).expect("UTF-8");
    assert!(
        saved == expected,
        "new_outputs.ipynb: {}",
        first_difference(&expected, &saved)
    );
    insta::assert_snapshot!("new-outputs-expected", saved);
}

#[test]
fn saving_does_not_change_the_model() {
    let notebook = replaced_notebook();
    let before = notebook.clone();
    let _ = serialize(&notebook);
    assert_eq!(before, notebook);
    let Cell::Code(code) = &notebook.cells[0] else {
        panic!("code cell");
    };
    assert!(code.outputs.iter().all(Output::is_new));

    let loaded: Output =
        serde_json::from_str(r#"{"output_type":"stream","name":"stdout","text":"x"}"#)
            .expect("deserializes");
    assert!(!loaded.is_new());
}

#[test]
fn reopening_a_saved_notebook_is_a_fixed_point() {
    let model = replaced_notebook();
    let text = serialize(&model);
    let reopened = parse(text.as_bytes()).expect("reparses");
    assert_eq!(serialize(&reopened), text);
    let Cell::Code(code) = &reopened.cells[0] else {
        panic!("code cell");
    };
    assert!(code.outputs.iter().all(|output| !output.is_new()));
}

#[test]
fn new_stream_splits_into_lines_but_reads_as_one_text() {
    let mut notebook = parse(&fixture("jupyter", "new_outputs.ipynb")).expect("parses");
    *code_outputs(&mut notebook, "new-outputs") = vec![Output::new_stream("stdout", "a\nb\n")];
    let value: Value = serde_json::from_str(&serialize(&notebook)).expect("output parses");
    assert_eq!(
        value.pointer("/cells/0/outputs/0/text"),
        Some(&serde_json::json!(["a\n", "b\n"]))
    );
    let Cell::Code(code) = &notebook.cells[0] else {
        panic!("code cell");
    };
    let Output::Stream(stream) = &code.outputs[0] else {
        panic!("stream output");
    };
    assert_eq!(stream.text.text(), "a\nb\n");
}

/// Check that the model holds the text of each new output as one string.
///
/// `MultilineText` serializes the form it holds, which tells one string from
/// an array. The CRLF also tells it from the normalized reading text.
fn assert_held_as_one_string(outputs: &[Output]) {
    let Output::Stream(stream) = &outputs[0] else {
        panic!("stream output");
    };
    assert_eq!(
        serde_json::to_value(&stream.text).expect("serializes"),
        serde_json::json!("a\r\nb\n")
    );
    let one_string = europa_core::JsonValue::from("x\ny");
    let Output::DisplayData(display) = &outputs[1] else {
        panic!("display data output");
    };
    assert_eq!(display.data.get("text/plain"), Some(&one_string));
    let Output::ExecuteResult(result) = &outputs[2] else {
        panic!("execute result output");
    };
    assert_eq!(result.data.get("text/plain"), Some(&one_string));
}

#[test]
fn new_outputs_keep_one_string_in_the_model() {
    let mut data = europa_core::JsonMap::new();
    data.insert("text/plain".to_owned(), "x\ny".into());
    let outputs = vec![
        Output::new_stream("stdout", "a\r\nb\n"),
        Output::new_display_data(data.clone(), europa_core::JsonMap::new()),
        Output::new_execute_result(data, europa_core::JsonMap::new(), Some(1)),
    ];
    assert_held_as_one_string(&outputs);
    let mut notebook = parse(&fixture("jupyter", "new_outputs.ipynb")).expect("parses");
    *code_outputs(&mut notebook, "new-outputs") = outputs;
    let value: Value = serde_json::from_str(&serialize(&notebook)).expect("output parses");
    assert_eq!(
        value.pointer("/cells/0/outputs/0/text"),
        Some(&serde_json::json!(["a\r\n", "b\n"]))
    );
    for idx in [1, 2] {
        assert_eq!(
            value.pointer(&format!("/cells/0/outputs/{idx}/data/text~1plain")),
            Some(&serde_json::json!(["x\n", "y"]))
        );
    }
    let Cell::Code(code) = &notebook.cells[0] else {
        panic!("code cell");
    };
    assert_held_as_one_string(&code.outputs);
}

#[test]
fn loaded_values_keep_their_form_next_to_new_outputs() {
    let mut notebook = parse(&fixture("foreign", "text_forms.ipynb")).expect("parses");
    let mut data = europa_core::JsonMap::new();
    data.insert("text/plain".to_owned(), "n1\nn2".into());
    let outputs = code_outputs(&mut notebook, "code-1");
    outputs.push(Output::new_stream("stderr", "e1\ne2"));
    outputs.push(Output::new_display_data(data, europa_core::JsonMap::new()));
    let text = serialize(&notebook);
    let value: Value = serde_json::from_str(&text).expect("output parses");
    assert_eq!(
        value.pointer("/cells/1/outputs/0/text"),
        Some(&serde_json::json!("a\nb\n"))
    );
    assert_eq!(
        value.pointer("/cells/1/outputs/1/data/text~1plain"),
        Some(&serde_json::json!("multi\nline"))
    );
    assert_eq!(
        value.pointer("/cells/1/outputs/2/text"),
        Some(&serde_json::json!(["e1\n", "e2"]))
    );
    assert_eq!(
        value.pointer("/cells/1/outputs/3/data/text~1plain"),
        Some(&serde_json::json!(["n1\n", "n2"]))
    );
    insta::assert_snapshot!("new-outputs-mixed", text);
}

#[test]
fn huge_new_stream_is_split_quickly() {
    let line = format!("{}\n", "y".repeat(79));
    let big = line.repeat(10 * 1024 * 1024 / line.len());
    let mut notebook = parse(&fixture("jupyter", "new_outputs.ipynb")).expect("parses");
    *code_outputs(&mut notebook, "new-outputs") = vec![Output::new_stream("stdout", big)];
    let started = Instant::now();
    let text = serialize(&notebook);
    let reopened = parse(text.as_bytes()).expect("reparses");
    assert!(started.elapsed() < Duration::from_secs(5));
    let value: Value = serde_json::from_str(&text).expect("output parses");
    let lines = value
        .pointer("/cells/0/outputs/0/text")
        .and_then(Value::as_array)
        .expect("stream text is an array");
    assert!(lines.len() > 100_000);
    assert_eq!(reopened.cells.len(), 1);
}
