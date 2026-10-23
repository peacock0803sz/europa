//! Boundary inputs that must load without rejection.

use europa_core::{parse, Cell, Output};

fn load(bytes: &[u8]) -> europa_core::Notebook {
    parse(bytes).expect("boundary input should load")
}

#[test]
fn accepts_empty_structures() {
    let notebook = load(br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[]}"#);
    assert!(notebook.cells.is_empty());
    let notebook = load(
        br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"code","source":"","metadata":{},"outputs":[],"execution_count":null}]}"#,
    );
    assert_eq!(notebook.cells.len(), 1);
    let notebook = load(
        br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"markdown","source":[],"metadata":{}}]}"#,
    );
    assert_eq!(notebook.cells.len(), 1);
}

#[test]
fn keeps_non_string_ids_without_rejecting() {
    for source_id in [r#"null"#, "123", r#""ok""#] {
        let bytes = format!(
            r#"{{"nbformat":4,"nbformat_minor":0,"metadata":{{}},"cells":[{{"cell_type":"markdown","id":{source_id},"source":"","metadata":{{}}}}]}}"#
        );
        let notebook = load(bytes.as_bytes());
        assert_eq!(notebook.cells.len(), 1);
    }
    let missing = load(
        br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"raw","source":"","metadata":{}}]}"#,
    );
    assert_eq!(missing.cells.len(), 1);
}

#[test]
fn loads_huge_outputs() {
    let big_text = "x".repeat(10 * 1024 * 1024);
    let bytes = format!(
        r#"{{"nbformat":4,"nbformat_minor":0,"metadata":{{}},"cells":[{{"cell_type":"code","source":"","metadata":{{}},"outputs":[{{"output_type":"stream","name":"stdout","text":"{big_text}"}}],"execution_count":1}}]}}"#
    );
    let notebook = load(bytes.as_bytes());
    assert_eq!(notebook.cells.len(), 1);

    let big_image = "A".repeat(10 * 1024 * 1024);
    let bytes = format!(
        r#"{{"nbformat":4,"nbformat_minor":0,"metadata":{{}},"cells":[{{"cell_type":"code","source":"","metadata":{{}},"outputs":[{{"output_type":"display_data","data":{{"image/png":"{big_image}"}},"metadata":{{}}}}],"execution_count":1}}]}}"#
    );
    let notebook = load(bytes.as_bytes());
    assert_eq!(notebook.cells.len(), 1);
}

#[test]
fn loads_unknown_kinds_and_ignores_extra_schema() {
    let notebook = load(
        br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"future","source":"","metadata":{},"attachments":{},"custom":1}]}"#,
    );
    assert!(matches!(notebook.cells[0], Cell::Unknown(_)));
    let notebook = load(
        br#"{"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[{"cell_type":"code","source":"","metadata":{},"outputs":[{"output_type":"future","x":1}],"execution_count":null}]}"#,
    );
    match &notebook.cells[0] {
        Cell::Code(code) => assert!(matches!(code.outputs[0], Output::Unknown(_))),
        other => panic!("expected code cell, got {other:?}"),
    }
}

/// Open, save and reopen once; the reopened model must equal the first.
fn write_back_time(bytes: &[u8]) -> std::time::Duration {
    let started = std::time::Instant::now();
    let first = load(bytes);
    let reopened = load(europa_core::serialize(&first).as_bytes());
    let elapsed = started.elapsed();
    assert_eq!(first, reopened);
    elapsed
}

#[test]
fn writes_back_huge_outputs() {
    let limit = std::time::Duration::from_secs(5);

    let big_text = "x".repeat(10 * 1024 * 1024);
    let bytes = format!(
        r#"{{"nbformat":4,"nbformat_minor":0,"metadata":{{}},"cells":[{{"cell_type":"code","source":"","metadata":{{}},"outputs":[{{"output_type":"stream","name":"stdout","text":"{big_text}"}}],"execution_count":1}}]}}"#
    );
    assert!(write_back_time(bytes.as_bytes()) < limit);

    let big_image = "A".repeat(10 * 1024 * 1024);
    let bytes = format!(
        r#"{{"nbformat":4,"nbformat_minor":0,"metadata":{{}},"cells":[{{"cell_type":"code","source":"","metadata":{{}},"outputs":[{{"output_type":"display_data","data":{{"image/png":"{big_image}"}},"metadata":{{}}}}],"execution_count":1}}]}}"#
    );
    assert!(write_back_time(bytes.as_bytes()) < limit);
}

#[test]
fn write_back_time_does_not_grow_with_nesting() {
    let big = "x".repeat(10 * 1024 * 1024);
    let depth = 120;
    let shallow =
        format!(r#"{{"nbformat":4,"nbformat_minor":0,"metadata":{{"k":"{big}"}},"cells":[]}}"#);
    let deep = format!(
        r#"{{"nbformat":4,"nbformat_minor":0,"metadata":{}"{big}"{},"cells":[]}}"#,
        r#"{"k":"#.repeat(depth),
        "}".repeat(depth)
    );
    let shallow_time = write_back_time(shallow.as_bytes());
    let deep_time = write_back_time(deep.as_bytes());
    // The floor keeps scheduler noise from failing a run when both are fast.
    let allowed = shallow_time.max(std::time::Duration::from_millis(100)) * 3;
    assert!(
        deep_time <= allowed,
        "deep {deep_time:?} vs shallow {shallow_time:?}"
    );
}
