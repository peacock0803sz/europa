//! Serialization format guards.

use europa_core::{parse, serialize};

// Key order and number spelling come from `europa-core` itself and no longer
// depend on `serde_json` features. `float_roundtrip` stays enabled so that
// validation rounds numbers near `f64::MAX` the same way
// `JsonNumber::from_literal` does. `arbitrary_precision` must stay disabled,
// also through feature unification with other crates, because validation
// would then accept literals that overflow `f64`, such as `1e400`. Do not
// delete or relax this test.
#[test]
fn writes_deterministic_format() {
    let input = r#"{"nbformat":4,"nbformat_minor":5,"metadata":{"b":"é","a":1},"cells":[{"cell_type":"markdown","metadata":{},"source":"héllo"}]}"#;
    let notebook = parse(input.as_bytes()).expect("fixture parses");
    let text = serialize(&notebook);

    assert!(text.ends_with('\n'), "trailing newline");
    assert!(!text.contains("\r\n"), "no CRLF");
    assert!(text.contains("\n \"cells\""), "one-space indent visible");

    let lines: Vec<&str> = text.lines().collect();
    let cells_idx = lines
        .iter()
        .position(|line| line.trim_start().starts_with("\"cells\""))
        .expect("cells key");
    let metadata_idx = lines
        .iter()
        .position(|line| line.trim_start().starts_with("\"metadata\""))
        .expect("metadata key");
    assert!(
        cells_idx < metadata_idx,
        "keys sort alphabetically at top level"
    );
    assert!(
        text.contains("é") && !text.contains("\\u00e9"),
        "non-ASCII stays unescaped"
    );

    let first: serde_json::Value = serde_json::from_str(&text).expect("output parses");
    let second_text = serialize(&parse(text.as_bytes()).expect("reparses"));
    assert_eq!(text, second_text, "formatting is stable");
    let _ = first;
    insta::assert_snapshot!("serialize-format", text);
}
