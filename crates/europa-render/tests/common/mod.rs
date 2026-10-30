#![allow(dead_code)]

use std::num::{NonZeroU16, NonZeroUsize};

use europa_core::{
    Background, Cell, JsonMap, Notebook, Output, RenderOptions, RenderedCell, Style, StyledLine,
};
use unicode_width::UnicodeWidthStr;

/// Options for an area `width` columns wide, with the default limit and a dark
/// background.
pub fn opts(width: u16) -> RenderOptions {
    RenderOptions::new(NonZeroU16::new(width).expect("width is not zero"))
}

/// Like [`opts`], but with the line limit `max`.
pub fn limited(width: u16, max: usize) -> RenderOptions {
    RenderOptions {
        max_output_lines: NonZeroUsize::new(max).expect("limit is not zero"),
        ..opts(width)
    }
}

/// Like [`opts`], but for a light background.
pub fn light(width: u16) -> RenderOptions {
    RenderOptions {
        background: Background::Light,
        ..opts(width)
    }
}

pub fn stdout(text: &str) -> Output {
    stream("stdout", text)
}

pub fn stderr(text: &str) -> Output {
    stream("stderr", text)
}

pub fn stream(name: &str, text: &str) -> Output {
    Output::new_stream(name, text)
}

/// Display data holding only `text/plain`.
pub fn plain(text: &str) -> Output {
    Output::new_display_data(
        JsonMap::from([("text/plain".to_owned(), text.into())]),
        JsonMap::new(),
    )
}

pub fn error(ename: &str, evalue: &str, traceback: &[&str]) -> Output {
    Output::new_error(
        ename,
        evalue,
        traceback.iter().map(|line| (*line).to_owned()).collect(),
    )
}

/// Outputs read from a JSON array in the shape nbformat saves them.
pub fn outputs(json: &str) -> Vec<Output> {
    serde_json::from_str(json).expect("outputs read")
}

/// The text of each display line.
pub fn texts(cell: &RenderedCell) -> Vec<String> {
    cell.lines
        .iter()
        .map(|line| line.spans.iter().map(|span| span.text.as_str()).collect())
        .collect()
}

/// The spans of a line as text and style.
pub fn styled(line: &StyledLine) -> Vec<(String, Style)> {
    line.spans
        .iter()
        .map(|span| (span.text.clone(), span.style))
        .collect()
}

/// The columns a line takes.
pub fn line_width(line: &StyledLine) -> usize {
    line.spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.text.as_str()))
        .sum()
}

/// Notebook with one code cell, `c1`, that holds the outputs in the JSON array.
pub fn notebook(outputs_json: &str) -> Notebook {
    let text = format!(
        r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{}},"cells":[{{"cell_type":"code","id":"c1","metadata":{{}},"source":"","outputs":{outputs_json},"execution_count":null}}]}}"#
    );
    europa_core::parse(text.as_bytes()).expect("fixture notebook loads")
}

/// The outputs of the cell `c1` of a notebook made by [`notebook`].
pub fn cell_outputs(notebook: &Notebook) -> &[Output] {
    let Some(Cell::Code(code)) = notebook.cells.iter().find(|cell| cell.id() == "c1") else {
        panic!("no code cell c1");
    };
    &code.outputs
}
