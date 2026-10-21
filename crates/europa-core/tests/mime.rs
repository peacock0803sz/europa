//! MIME bundle values stay untouched.

use europa_core::{parse, Cell, Output};
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
    for dimensions in [
        r#"{"width":"wide","height":-3}"#,
        r#"{"width":null,"height":"100px"}"#,
    ] {
        let bytes = format!(
            r#"{{"nbformat":4,"nbformat_minor":0,"metadata":{{}},"cells":[{{"cell_type":"code","source":"","metadata":{{}},"outputs":[{{"output_type":"display_data","data":{{"image/png":"aGVsbG8="}},"metadata":{dimensions}}}],"execution_count":null}}]}}"#
        );
        let notebook = code_cell(bytes.as_bytes());
        match &notebook.cells[0] {
            Cell::Code(code) => match &code.outputs[0] {
                Output::DisplayData(data) => {
                    assert!(data.metadata.get("width").is_some());
                    assert!(data.metadata.get("height").is_some());
                }
                other => panic!("expected display data, got {other:?}"),
            },
            other => panic!("expected code cell, got {other:?}"),
        }
    }
}
