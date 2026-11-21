//! Cell display lines.

use std::num::NonZeroU16;

use europa_core::{RenderOptions, SourceLanguage};

use crate::tests::common;

fn opts() -> RenderOptions {
    RenderOptions::new(NonZeroU16::new(40).expect("non-zero"))
}

fn texts(cell: &europa_core::RenderedCell) -> Vec<String> {
    cell.lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.text.as_str()).collect())
        .collect()
}

#[test]
fn code_border_source_separator_and_output() {
    let notebook = common::notebook(&[common::code_cell(
        "a = 1\nb = 2\n",
        Some(12),
        &[common::stream("stdout", "ok\n")],
    )]);
    let options = opts();
    let mut sinks = 0;
    let cell = crate::cell_display::cell_display(
        &notebook.cells[0],
        3,
        SourceLanguage::Python,
        &options,
        &mut |_, _| sinks += 1,
    );
    let lines = texts(&cell);
    assert_eq!(lines.len(), 5);
    assert!(lines[0].starts_with("== 3 code [12] "));
    assert_eq!(lines[0].chars().count(), 40);
    assert!(lines[0].ends_with("="));
    assert!(cell.lines[0].spans.len() == 1 && cell.lines[0].spans[0].style.bold);
    assert_eq!(&lines[1..3], &["a = 1", "b = 2"]);
    assert!(lines[3].starts_with("-- out "));
    assert_eq!(lines[3].chars().count(), 40);
    assert!(cell.lines[3].spans.iter().all(|s| s.style.dim));
    assert_eq!(lines[4], "ok");
    assert_eq!(sinks, 0);
}

#[test]
fn missing_execution_count_shows_blank() {
    let notebook = common::notebook(&[common::code_cell("x\n", None, &[])]);
    let options = opts();
    let cell = crate::cell_display::cell_display(
        &notebook.cells[0],
        1,
        SourceLanguage::Python,
        &options,
        &mut |_, _| {},
    );
    assert!(texts(&cell)[0].starts_with("== 1 code [ ]"));
}

#[test]
fn empty_and_blank_outputs_have_no_separator() {
    for outputs in [vec![], vec![common::stream("stdout", "")]] {
        let notebook = common::notebook(&[common::code_cell("x\n", Some(1), &outputs)]);
        let options = opts();
        let cell = crate::cell_display::cell_display(
            &notebook.cells[0],
            1,
            SourceLanguage::Python,
            &options,
            &mut |_, _| {},
        );
        let lines = texts(&cell);
        assert!(!lines.iter().any(|l| l.starts_with("-- out")), "{lines:?}");
    }
}

#[test]
fn empty_code_cell_is_only_a_border() {
    let notebook = common::notebook(&[common::code_cell("", Some(1), &[])]);
    let options = opts();
    let cell = crate::cell_display::cell_display(
        &notebook.cells[0],
        1,
        SourceLanguage::Python,
        &options,
        &mut |_, _| {},
    );
    assert_eq!(texts(&cell).len(), 1);
}

#[test]
fn markdown_and_raw_sources_match_renderers() {
    let options = opts();
    let markdown = common::notebook(&[common::markdown_cell("# T\n\nbody\n")]);
    let cell = crate::cell_display::cell_display(
        &markdown.cells[0],
        2,
        SourceLanguage::Python,
        &options,
        &mut |_, _| {},
    );
    let lines = texts(&cell);
    assert!(lines[0].starts_with("== 2 markdown "));
    let expected = europa_render::render_markdown("# T\n\nbody\n", &options);
    let shown: Vec<String> = expected
        .iter()
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect())
        .collect();
    assert_eq!(&lines[1..], &shown);
    let raw = common::notebook(&[common::raw_cell("plain\ntext\n")]);
    let cell = crate::cell_display::cell_display(
        &raw.cells[0],
        2,
        SourceLanguage::Python,
        &options,
        &mut |_, _| {},
    );
    let lines = texts(&cell);
    assert!(lines[0].starts_with("== 2 raw "));
    let expected = europa_render::render_source("plain\ntext\n", SourceLanguage::Plain, &options);
    let shown: Vec<String> = expected
        .iter()
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect())
        .collect();
    assert_eq!(&lines[1..], &shown);
}

#[test]
fn julia_code_is_plain() {
    let text = "{\"nbformat\":4,\"nbformat_minor\":5,\"metadata\":{\"language_info\":{\"name\":\"julia\"}},\"cells\":[{\"cell_type\":\"code\",\"metadata\":{},\"source\":\"x = 1\\n\",\"outputs\":[],\"execution_count\":1}]}";
    let notebook = europa_core::parse(text.as_bytes()).expect("loads");
    let options = opts();
    let cell = crate::cell_display::cell_display(
        &notebook.cells[0],
        1,
        notebook.source_language(),
        &options,
        &mut |_, _| {},
    );
    let expected = europa_render::render_source("x = 1\n", SourceLanguage::Plain, &options);
    let shown: Vec<String> = cell.lines[1..]
        .iter()
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect())
        .collect();
    let wanted: Vec<String> = expected
        .iter()
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect())
        .collect();
    assert_eq!(shown, wanted);
}

#[test]
fn unknown_kind_shows_a_notice() {
    let notebook = common::notebook(&[common::unknown_cell("widget")]);
    let options = opts();
    let cell = crate::cell_display::cell_display(
        &notebook.cells[0],
        4,
        SourceLanguage::Python,
        &options,
        &mut |_, _| {},
    );
    let lines = texts(&cell);
    assert!(lines[0].starts_with("== 4 ? "));
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("[unknown cell type: widget]"));
    assert!(cell.lines[1].spans.iter().all(|s| s.style.dim));
}

#[test]
fn control_characters_in_kind_become_pictures() {
    let notebook = common::notebook(&[common::unknown_cell("\u{1b}[31mx")]);
    let options = opts();
    let cell = crate::cell_display::cell_display(
        &notebook.cells[0],
        1,
        SourceLanguage::Python,
        &options,
        &mut |_, _| {},
    );
    let joined: String = texts(&cell).concat();
    assert!(joined.contains("␛"), "{joined:?}");
    assert!(!joined.contains("\u{1b}"), "{joined:?}");
}

#[test]
fn image_slot_moves_after_leading_lines() {
    let bytes = common::png(64, 40);
    let data = base64_simd::STANDARD.encode_to_string(&bytes);
    let output = format!(
        "{{\"output_type\":\"display_data\",\"data\":{{\"image/png\":\"{data}\"}},\"metadata\":{{}}}}"
    );
    let cell_source = common::code_cell("a\nb\n", Some(1), std::slice::from_ref(&output));
    let notebook = common::notebook(std::slice::from_ref(&cell_source));
    let options = opts();
    let mut seen = 0;
    let cell = crate::cell_display::cell_display(
        &notebook.cells[0],
        1,
        SourceLanguage::Python,
        &options,
        &mut |_, _| seen += 1,
    );
    assert_eq!(seen, 1);
    assert_eq!(cell.images.len(), 1);
    assert_eq!(cell.images[0].output, 0);
    let only = europa_render::render_outputs(&[common::png_output(&bytes)], &options);
    assert_eq!(cell.images[0].line, only.images[0].line + 4);
}

#[test]
fn hidden_metadata_changes_nothing() {
    let plain = common::code_cell("x\n", Some(1), &[common::stream("stdout", "ok\n")]);
    let out = common::stream("stdout", "ok\n");
    let text = format!(
        "{{\"nbformat\":4,\"nbformat_minor\":5,\"metadata\":{{}},\"cells\":[{{\"cell_type\":\"code\",\"metadata\":{{\"jupyter\":{{\"source_hidden\":true,\"outputs_hidden\":true}},\"collapsed\":true}},\"source\":\"x\\n\",\"outputs\":[{out}],\"execution_count\":1}}]}}",
    );
    let a = common::notebook(&[plain]);
    let b = europa_core::parse(text.as_bytes()).expect("loads");
    let options = opts();
    let left = crate::cell_display::cell_display(
        &a.cells[0],
        1,
        SourceLanguage::Python,
        &options,
        &mut |_, _| {},
    );
    let right = crate::cell_display::cell_display(
        &b.cells[0],
        1,
        SourceLanguage::Python,
        &options,
        &mut |_, _| {},
    );
    assert_eq!(texts(&left), texts(&right));
}

#[test]
fn narrow_widths_do_not_fail() {
    for width in [1, 2] {
        let options = RenderOptions::new(NonZeroU16::new(width).expect("non-zero"));
        let notebook = common::notebook(&[common::code_cell("x\n", Some(1), &[])]);
        let cell = crate::cell_display::cell_display(
            &notebook.cells[0],
            1,
            SourceLanguage::Python,
            &options,
            &mut |_, _| {},
        );
        let border: String = cell.lines[0]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(border, if width == 1 { "=" } else { "==" }, "width {width}");
    }
}
