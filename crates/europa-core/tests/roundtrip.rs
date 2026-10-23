//! Round-trip preservation tests.

use std::path::PathBuf;

use europa_core::{parse, serialize};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("roundtrip")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|_| panic!("missing fixture {name}"))
}

fn roundtrip(name: &str) -> (europa_core::Notebook, String) {
    let bytes = fixture(name);
    let first = parse(&bytes).unwrap_or_else(|error| panic!("{name} parses: {error:?}"));
    let text = serialize(&first);
    let second =
        parse(text.as_bytes()).unwrap_or_else(|error| panic!("{name} reparses: {error:?}"));
    assert_eq!(first, second, "{name} model equality");
    let again = serialize(&second);
    assert_eq!(text, again, "{name} output stability");
    (first, text)
}

#[test]
fn preserves_unknown_fields() {
    for name in [
        "unknown_top.ipynb",
        "unknown_cell.ipynb",
        "unknown_output.ipynb",
        "unknown_all_layers.ipynb",
        "attachments.ipynb",
    ] {
        let (notebook, text) = roundtrip(name);
        let value: serde_json::Value =
            serde_json::from_str(&text).expect("serialized output parses");
        match name {
            "unknown_top.ipynb" => {
                assert!(value.get("custom_top").is_some());
            }
            "attachments.ipynb" => {
                assert!(value.pointer("/cells/0/attachments").is_some());
            }
            _ => {}
        }
        insta::assert_snapshot!(format!("roundtrip-{name}"), text);
        let _ = notebook;
    }
}

#[test]
fn keeps_source_form_and_ids() {
    let bytes = fixture("array_crlf.ipynb");
    let raw: serde_json::Value = serde_json::from_slice(&bytes).expect("fixture parses as JSON");
    let (notebook, text) = roundtrip("array_crlf.ipynb");
    let out: serde_json::Value = serde_json::from_str(&text).expect("output parses as JSON");
    assert_eq!(
        raw.pointer("/cells/0/source"),
        out.pointer("/cells/0/source"),
        "array source unchanged"
    );
    assert_eq!(
        raw.pointer("/cells/1/source"),
        out.pointer("/cells/1/source"),
        "array source with CRLF unchanged"
    );
    let _ = notebook;

    let bytes = fixture("string_crlf.ipynb");
    let raw: serde_json::Value = serde_json::from_slice(&bytes).expect("fixture parses as JSON");
    let (_, text) = roundtrip("string_crlf.ipynb");
    let out: serde_json::Value = serde_json::from_str(&text).expect("output parses as JSON");
    assert_eq!(
        out.pointer("/cells/0/source"),
        Some(&serde_json::json!("a\r\nb")),
        "string source with CRLF unchanged"
    );
    assert_eq!(
        raw.pointer("/cells/0/source"),
        out.pointer("/cells/0/source")
    );
    assert_eq!(
        out.pointer("/cells/0/outputs/0/text"),
        Some(&serde_json::json!("out\r\nmore")),
        "string stream text with CRLF unchanged"
    );
    assert_eq!(
        raw.pointer("/cells/0/outputs/0/text"),
        out.pointer("/cells/0/outputs/0/text")
    );

    let (_, text) = roundtrip("ids.ipynb");
    let out: serde_json::Value = serde_json::from_str(&text).expect("output parses as JSON");
    let cells = out
        .get("cells")
        .and_then(|cells| cells.as_array())
        .expect("cells");
    assert_eq!(cells.len(), 5);
    assert!(cells[0].get("id").is_none(), "missing id stays missing");
    assert_eq!(
        cells[1].get("id"),
        Some(&serde_json::Value::String("dup".to_owned()))
    );
    assert_eq!(
        cells[2].get("id"),
        Some(&serde_json::Value::String("dup".to_owned()))
    );
    assert_eq!(cells[3].get("id"), Some(&serde_json::Value::Null));
    assert_eq!(cells[4].get("id"), Some(&serde_json::json!(123)));
}

#[test]
fn keeps_unknown_kinds_and_null_execution() {
    use europa_core::{Cell, Output};

    let (notebook, text) = roundtrip("unknown_kind.ipynb");
    assert!(matches!(notebook.cells[0], Cell::Unknown(_)));
    match &notebook.cells[1] {
        Cell::Code(code) => assert!(matches!(code.outputs[0], Output::Unknown(_))),
        other => panic!("expected code cell, got {other:?}"),
    }
    let out: serde_json::Value = serde_json::from_str(&text).expect("output parses");
    assert_eq!(
        out.pointer("/cells/0/cell_type"),
        Some(&serde_json::Value::String("future-cell".to_owned()))
    );

    let (_, text) = roundtrip("execution_null.ipynb");
    let out: serde_json::Value = serde_json::from_str(&text).expect("output parses");
    assert_eq!(
        out.pointer("/cells/0/execution_count"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(
        out.pointer("/cells/0/outputs/0/execution_count"),
        Some(&serde_json::Value::Null)
    );
}

// Numbers are written back with the spelling they were read with, so these
// assertions pin the literal on each output line. Key order and number
// spelling no longer depend on `serde_json` features. `float_roundtrip` stays
// enabled so that validation rounds numbers near `f64::MAX` the same way
// `JsonNumber::from_literal` does. `arbitrary_precision` must stay disabled,
// also through feature unification with other crates, because validation
// would then accept literals that overflow `f64`, such as `1e400`.
#[test]
fn pins_number_roundtrip() {
    let (_, text) = roundtrip("numbers.ipynb");
    for line in [
        r#""one_point_zero": 1.0"#,
        r#""seventeen": 1.2345678901234567"#,
        r#""max_float": 1.7976931348623157e308"#,
        r#""min_normal": 2.2250738585072014e-308"#,
        r#""big_u64": 18446744073709551615"#,
        r#""beyond_u64": 18446744073709551616"#,
        r#""precise_beyond": 1.00000000000000011102230246251565404236316680908203125"#,
    ] {
        assert!(
            text.lines()
                .any(|found| found.trim().trim_end_matches(',') == line),
            "missing line {line} in:\n{text}"
        );
    }
    insta::assert_snapshot!("roundtrip-numbers", text);
}
