//! Rejection tests for invalid notebooks.

use europa_core::{parse, ParseError};

fn invalid_json(bytes: &[u8]) -> (usize, usize) {
    match parse(bytes) {
        Err(ParseError::InvalidJson { line, column, .. }) => (line, column),
        other => panic!("expected invalid JSON, got {other:?}"),
    }
}

fn invalid_nbformat(bytes: &[u8]) -> String {
    match parse(bytes) {
        Err(ParseError::InvalidNbformat { path, .. }) => path,
        other => panic!("expected invalid nbformat, got {other:?}"),
    }
}

fn invalid_minor(bytes: &[u8]) -> String {
    match parse(bytes) {
        Err(ParseError::InvalidNbformatMinor { path, .. }) => path,
        other => panic!("expected invalid nbformat_minor, got {other:?}"),
    }
}

fn mismatch(bytes: &[u8]) -> String {
    match parse(bytes) {
        Err(ParseError::Mismatch { path, .. }) => path,
        other => panic!("expected structural mismatch, got {other:?}"),
    }
}

fn minimal_notebook(nbformat: &str, minor: &str) -> Vec<u8> {
    format!("{{\"nbformat\":{nbformat},\"nbformat_minor\":{minor},\"metadata\":{{}},\"cells\":[]}}")
        .into_bytes()
}

#[test]
fn rejects_malformed_json() {
    let (line, column) = invalid_json(b"{");
    assert_eq!((line, column), (1, 1));
    insta::assert_snapshot!("malformed-json", format!("{:?}", parse(b"{").unwrap_err()));
}

#[test]
fn rejects_missing_nbformat_before_other_errors() {
    let bytes = b"{\"nbformat_minor\":0,\"metadata\":{},\"cells\":\"nope\"}";
    assert_eq!(invalid_nbformat(bytes), "/nbformat");
}

#[test]
fn rejects_non_integer_and_wrong_nbformat() {
    for literal in [r#""4""#, "4.0", "4e0", "-4", "3", "5", "null"] {
        let bytes = minimal_notebook(literal, "0");
        assert_eq!(invalid_nbformat(&bytes), "/nbformat", "literal {literal}");
    }
}

#[test]
fn rejects_missing_and_invalid_minor() {
    let bytes = br#"{"nbformat":4,"metadata":{},"cells":[]}"#.to_vec();
    assert_eq!(invalid_minor(&bytes), "/nbformat_minor");
    for literal in ["-1", "1.0", "4294967296", r#""0""#, "null"] {
        let bytes = minimal_notebook("4", literal);
        assert_eq!(
            invalid_minor(&bytes),
            "/nbformat_minor",
            "literal {literal}"
        );
    }
}

#[test]
fn rejects_non_object_root_and_bad_cells() {
    assert_eq!(mismatch(b"[]"), "");
    assert_eq!(
        mismatch(br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[1]}"#),
        "/cells/0"
    );
    assert_eq!(
        mismatch(br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{}]}"#),
        "/cells/0/cell_type"
    );
    assert_eq!(
        mismatch(
            br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":123,"source":"","metadata":{}}]}"#
        ),
        "/cells/0/cell_type"
    );
}

#[test]
fn rejects_missing_execution_count_and_out_of_range() {
    let missing = br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"code","source":"","metadata":{},"outputs":[]}]}"#;
    assert_eq!(mismatch(missing), "/cells/0/execution_count");
    let big = br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"code","source":"","metadata":{},"outputs":[],"execution_count":9223372036854775808}]}"#;
    assert_eq!(mismatch(big), "/cells/0/execution_count");
}

#[test]
fn rejects_output_type_problems() {
    let missing = br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"code","source":"","metadata":{},"outputs":[{}],"execution_count":null}]}"#;
    assert_eq!(mismatch(missing), "/cells/0/outputs/0/output_type");
    let bad = br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"code","source":"","metadata":{},"outputs":[{"output_type":1}],"execution_count":null}]}"#;
    assert_eq!(mismatch(bad), "/cells/0/outputs/0/output_type");
}

#[test]
fn rejects_required_field_gaps() {
    let no_meta = br#"{"nbformat":4,"nbformat_minor":0,"cells":[]}"#;
    assert_eq!(mismatch(no_meta), "/metadata");
    let code_no_outputs = br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"code","source":"","metadata":{},"execution_count":null}]}"#;
    assert_eq!(mismatch(code_no_outputs), "/cells/0/outputs");
    let stream_no_text = br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"code","source":"","metadata":{},"outputs":[{"output_type":"stream","name":"stdout"}],"execution_count":null}]}"#;
    assert_eq!(mismatch(stream_no_text), "/cells/0/outputs/0/text");
}

/// Notebook whose nesting depth is exactly `depth`, counting the root as 1.
fn nested_notebook(depth: usize) -> String {
    let inner = depth - 1;
    let mut text = String::from(r#"{"nbformat":4,"nbformat_minor":0,"metadata":"#);
    for _ in 0..inner - 1 {
        text.push_str(r#"{"a":"#);
    }
    text.push_str("{}");
    for _ in 0..inner - 1 {
        text.push('}');
    }
    text.push_str(r#","cells":[]}"#);
    text
}

#[test]
fn accepts_nesting_boundary_and_rejects_beyond() {
    let ok = nested_notebook(127);
    let parsed = parse(ok.as_bytes());
    assert!(parsed.is_ok(), "127 levels should load: {parsed:?}");

    let deep = nested_notebook(128);
    assert!(matches!(
        parse(deep.as_bytes()),
        Err(ParseError::InvalidJson { .. })
    ));
}

#[test]
fn rejects_byte_order_mark_and_bad_encoding() {
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[]}"#);
    assert_eq!(invalid_json(&bom), (1, 1));
    let bad = [0xFF, 0xFE, b'{', b'}'];
    let (line, _) = invalid_json(&bad);
    assert_eq!(line, 1);
    let lone = br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"markdown","source":"\ud800","metadata":{}}]}"#;
    assert!(matches!(parse(lone), Err(ParseError::InvalidJson { .. })));
    let dup = br#"{"nbformat":4,"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[]}"#;
    assert!(matches!(parse(dup), Err(ParseError::InvalidJson { .. })));
    insta::assert_snapshot!(
        "reject-byte-order-mark",
        format!("{:?}", parse(&bom).unwrap_err())
    );
}

fn with_metadata(metadata: &str) -> Vec<u8> {
    format!(r#"{{"nbformat":4,"nbformat_minor":0,"metadata":{metadata},"cells":[]}}"#).into_bytes()
}

/// Builds the JSON escape sequence (backslash, `u`, four hex digits) for
/// `hex` at runtime so the test source never contains a literal escape.
fn esc(hex: &str) -> String {
    format!("\\{}{}", 'u', hex)
}

#[test]
fn accepts_distinct_escaped_non_ascii_keys() {
    // Equal-length escapes that decode to different characters must not
    // collide in duplicate-key detection.
    let metadata = format!(
        r#"{{"{}":1,"{}":2,"{}{}":3}}"#,
        esc("00e9"),
        esc("00e8"),
        esc("65e5"),
        esc("672c")
    );
    let bytes = with_metadata(&metadata);
    assert!(parse(&bytes).is_ok(), "metadata {metadata}");
}

#[test]
fn rejects_duplicate_keys_regardless_of_spelling() {
    for metadata in [
        format!(r#"{{"{}":1,"{}":2}}"#, esc("00e9"), '\u{e9}'),
        format!(r#"{{"{}":1,"{}":2}}"#, esc("00e9"), esc("00e9")),
        format!(r#"{{"a":1,"{}":2}}"#, esc("0061")),
        r#"{"a":1,"a":2}"#.to_owned(),
        r#"{"outer":{"k":1,"k":2}}"#.to_owned(),
    ] {
        let bytes = with_metadata(&metadata);
        assert!(
            matches!(parse(&bytes), Err(ParseError::InvalidJson { .. })),
            "metadata {metadata}"
        );
    }
}

fn with_cells(cells: &str) -> Vec<u8> {
    format!(r#"{{"nbformat":4,"nbformat_minor":0,"metadata":{{}},"cells":{cells}}}"#).into_bytes()
}

fn with_output(output: &str) -> Vec<u8> {
    with_cells(&format!(
        r#"[{{"cell_type":"code","source":"","metadata":{{}},"outputs":[{output}],"execution_count":null}}]"#
    ))
}

#[test]
fn reports_required_field_paths() {
    let cases = [
        (with_cells(r#"{}"#), "/cells"),
        (
            br#"{"nbformat":4,"nbformat_minor":0,"metadata":[],"cells":[]}"#.to_vec(),
            "/metadata",
        ),
        (
            with_cells(r#"[{"cell_type":"markdown","source":1,"metadata":{}}]"#),
            "/cells/0/source",
        ),
        (
            with_cells(r#"[{"cell_type":"raw","source":["a",1],"metadata":{}}]"#),
            "/cells/0/source",
        ),
        (
            with_cells(r#"[{"cell_type":"markdown","source":""}]"#),
            "/cells/0/metadata",
        ),
        (
            with_output(r#"{"output_type":"display_data","metadata":{}}"#),
            "/cells/0/outputs/0/data",
        ),
        (
            with_output(r#"{"output_type":"execute_result","data":{},"metadata":{}}"#),
            "/cells/0/outputs/0/execution_count",
        ),
        (
            with_output(r#"{"output_type":"error","ename":"E","evalue":"v"}"#),
            "/cells/0/outputs/0/traceback",
        ),
        (
            with_output(r#"{"output_type":"error","ename":"E","evalue":"v","traceback":["a",1]}"#),
            "/cells/0/outputs/0/traceback",
        ),
    ];
    for (bytes, path) in cases {
        assert_eq!(
            mismatch(&bytes),
            path,
            "input {}",
            String::from_utf8_lossy(&bytes)
        );
    }
}

#[test]
fn checks_surrogate_pairing() {
    let source = |literal: &str| {
        with_cells(&format!(
            r#"[{{"cell_type":"markdown","source":"{literal}","metadata":{{}}}}]"#
        ))
    };
    let pair = format!("{}{}", esc("d83d"), esc("de00"));
    assert!(parse(&source(&pair)).is_ok(), "valid pair");
    assert!(
        matches!(
            parse(&source(r"\udc00")),
            Err(ParseError::InvalidJson { .. })
        ),
        "lone low surrogate"
    );
    assert!(
        parse(&source(r"\\ud800")).is_ok(),
        "escaped backslash is not an escape"
    );
}
