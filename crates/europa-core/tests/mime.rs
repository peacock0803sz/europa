//! MIME bundle values stay untouched.

use europa_core::{parse, serialize, Cell, Output};
use serde_json::Value;

fn code_cell(bytes: &[u8]) -> europa_core::Notebook {
    parse(bytes).expect("MIME fixture should load")
}

#[test]
fn preserves_mime_values() {
    let notebook = code_cell(
        br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"code","source":"","metadata":{},"outputs":[{"output_type":"execute_result","data":{"application/json":{"a":[1,2]}},"metadata":{},"execution_count":1}],"execution_count":1}]}"#,
    );
    match &notebook.cells[0] {
        Cell::Code(code) => match &code.outputs[0] {
            Output::ExecuteResult(result) => {
                assert_eq!(
                    result.data.get("application/json"),
                    Some(&serde_json::json!({"a": [1, 2]}))
                );
                insta::assert_snapshot!(
                    "mime-application-json",
                    serde_json::to_string_pretty(&result.data).unwrap()
                );
            }
            other => panic!("expected execute result, got {other:?}"),
        },
        other => panic!("expected code cell, got {other:?}"),
    }

    for (mime, raw) in [
        ("image/png", r#""aGVsbG8=""#),
        ("image/jpeg", r#""aGVsbG8=""#),
        ("image/gif", r#""aGVsbG8=""#),
    ] {
        let bytes = format!(
            r#"{{"nbformat":4,"nbformat_minor":0,"metadata":{{}},"cells":[{{"cell_type":"code","source":"","metadata":{{}},"outputs":[{{"output_type":"display_data","data":{{"{mime}":{raw}}},"metadata":{{}}}}],"execution_count":null}}]}}"#
        );
        let notebook = code_cell(bytes.as_bytes());
        match &notebook.cells[0] {
            Cell::Code(code) => match &code.outputs[0] {
                Output::DisplayData(data) => {
                    assert_eq!(
                        data.data.get(mime),
                        Some(&Value::String("aGVsbG8=".to_owned()))
                    );
                }
                other => panic!("expected display data, got {other:?}"),
            },
            other => panic!("expected code cell, got {other:?}"),
        }
    }

    let notebook = code_cell(
        br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"code","source":"","metadata":{},"outputs":[{"output_type":"display_data","data":{"image/svg+xml":["<svg>","</svg>"]},"metadata":{}}],"execution_count":null}]}"#,
    );
    match &notebook.cells[0] {
        Cell::Code(code) => match &code.outputs[0] {
            Output::DisplayData(data) => {
                assert_eq!(
                    data.data.get("image/svg+xml"),
                    Some(&serde_json::json!(["<svg>", "</svg>"]))
                );
                insta::assert_snapshot!(
                    "mime-bundle",
                    serde_json::to_string_pretty(&data.data).unwrap()
                );
            }
            other => panic!("expected display data, got {other:?}"),
        },
        other => panic!("expected code cell, got {other:?}"),
    }
}

#[test]
fn keeps_invalid_dimensions_without_failing() {
    for (dimensions, width, height) in [
        (
            r#"{"width":"wide","height":-3}"#,
            serde_json::json!("wide"),
            serde_json::json!(-3),
        ),
        (
            r#"{"width":null,"height":"100px"}"#,
            Value::Null,
            serde_json::json!("100px"),
        ),
        (
            r#"{"width":640,"height":480}"#,
            serde_json::json!(640),
            serde_json::json!(480),
        ),
    ] {
        let bytes = format!(
            r#"{{"nbformat":4,"nbformat_minor":0,"metadata":{{}},"cells":[{{"cell_type":"code","source":"","metadata":{{}},"outputs":[{{"output_type":"display_data","data":{{"image/png":"aGVsbG8="}},"metadata":{dimensions}}}],"execution_count":null}}]}}"#
        );
        let notebook = code_cell(bytes.as_bytes());
        match &notebook.cells[0] {
            Cell::Code(code) => match &code.outputs[0] {
                Output::DisplayData(data) => {
                    assert_eq!(data.metadata.get("width"), Some(&width), "{dimensions}");
                    assert_eq!(data.metadata.get("height"), Some(&height), "{dimensions}");
                }
                other => panic!("expected display data, got {other:?}"),
            },
            other => panic!("expected code cell, got {other:?}"),
        }
        let out: Value =
            serde_json::from_str(&serialize(&notebook)).expect("serialized output parses");
        let meta = out
            .pointer("/cells/0/outputs/0/metadata")
            .expect("output metadata");
        assert_eq!(meta.get("width"), Some(&width), "{dimensions} round trip");
        assert_eq!(meta.get("height"), Some(&height), "{dimensions} round trip");
    }
}
