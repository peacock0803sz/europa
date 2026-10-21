//! Normalized text checks.

use std::path::PathBuf;

use europa_core::{parse, Cell, Output};

fn load(name: &str) -> europa_core::Notebook {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("text_and_ids")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("missing fixture {name}"));
    parse(&bytes).unwrap_or_else(|error| panic!("{name} parses: {error:?}"))
}

fn cell_source(cell: &Cell) -> &str {
    match cell {
        Cell::Code(inner) => inner.source.text(),
        Cell::Markdown(inner) => inner.source.text(),
        Cell::Raw(inner) => inner.source.text(),
        Cell::Unknown(_) => panic!("unexpected unknown cell"),
    }
}

#[test]
fn normalizes_mixed_sources() {
    let notebook = load("mixed_source.ipynb");
    assert_eq!(cell_source(&notebook.cells[0]), "hello\nworld");
    assert_eq!(cell_source(&notebook.cells[1]), "hello\nworld");
    assert_eq!(cell_source(&notebook.cells[2]), "x");
    insta::assert_snapshot!(
        "normalize-mixed",
        notebook
            .cells
            .iter()
            .map(cell_source)
            .collect::<Vec<_>>()
            .join("---")
    );
}

#[test]
fn joins_split_crlf_and_keeps_lone_cr() {
    let notebook = load("crlf_split.ipynb");
    assert_eq!(cell_source(&notebook.cells[0]), "a\nb");
    assert_eq!(cell_source(&notebook.cells[1]), "x\ny");

    let notebook = load("lone_cr.ipynb");
    assert_eq!(cell_source(&notebook.cells[0]), "a\rb");
    assert_eq!(cell_source(&notebook.cells[1]), "x\ry");
}

#[test]
fn normalizes_stream_text() {
    let notebook = load("stream_text.ipynb");
    match &notebook.cells[0] {
        Cell::Code(code) => {
            match &code.outputs[0] {
                Output::Stream(stream) => assert_eq!(stream.text.text(), "a\nb"),
                other => panic!("expected stream, got {other:?}"),
            }
            match &code.outputs[1] {
                Output::Stream(stream) => assert_eq!(stream.text.text(), "x\ry"),
                other => panic!("expected stream, got {other:?}"),
            }
        }
        other => panic!("expected code cell, got {other:?}"),
    }
}
