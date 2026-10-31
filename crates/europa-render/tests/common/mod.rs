#![allow(dead_code)]

use std::num::{NonZeroU16, NonZeroUsize};

use europa_core::{
    Background, Cell, Color, JsonMap, Notebook, Output, RenderOptions, RenderedCell, Style,
    StyledLine,
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

/// Display data holding the MIME bundle `data` as nbformat saves it.
///
/// The bundle goes through [`europa_core::parse`], because `serde_json` would
/// read numbers as `f64` and change their text.
pub fn display(data: &str) -> Output {
    let json = format!(r#"[{{"output_type":"display_data","data":{data},"metadata":{{}}}}]"#);
    let notebook = notebook(&json);
    cell_outputs(&notebook)[0].clone()
}

/// Execute result holding the MIME bundle `data` as nbformat saves it.
///
/// The bundle goes through [`europa_core::parse`], because `serde_json` would
/// read numbers as `f64` and change their text.
pub fn result(data: &str) -> Output {
    let json = format!(
        r#"[{{"output_type":"execute_result","data":{data},"metadata":{{}},"execution_count":1}}]"#
    );
    let notebook = notebook(&json);
    cell_outputs(&notebook)[0].clone()
}

/// Text and background of each span of the line.
pub fn code_bg(line: &StyledLine) -> Vec<(String, Option<Color>)> {
    line.spans
        .iter()
        .map(|span| (span.text.clone(), span.style.bg))
        .collect()
}

/// Check that every span holds text and neighbours differ in decoration.
pub fn assert_lines_are_well_formed(cell: &RenderedCell) {
    for (index, line) in cell.lines.iter().enumerate() {
        for span in &line.spans {
            assert!(!span.text.is_empty(), "line {index} holds an empty span");
        }
        for pair in line.spans.windows(2) {
            assert!(
                pair[0].style != pair[1].style,
                "line {index} holds adjacent spans with the same decoration"
            );
        }
    }
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

/// What a fixture file holds.
#[derive(serde::Deserialize)]
struct Fixture {
    options: RenderOptions,
    outputs: Box<serde_json::value::RawValue>,
}

/// The `outputs` array of the file `tests/fixtures/{name}` as nbformat saves it.
pub fn fixture_outputs_json(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let fixture: Fixture =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{path}: {error}"));
    fixture.outputs.get().to_owned()
}

/// The rendering conditions and the outputs of the file `tests/fixtures/{name}`.
pub fn fixture(name: &str) -> (RenderOptions, Vec<Output>) {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let fixture: Fixture =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{path}: {error}"));
    let notebook = notebook(fixture.outputs.get());
    (fixture.options, cell_outputs(&notebook).to_vec())
}

/// Draw the cell on a test screen and describe the screen.
///
/// The screen is as wide as the render width and as high as the cell. The
/// description is the options on one line, then the buffer with the styles
/// that changed along it, so a color or a fill that stops short shows in a
/// snapshot.
pub fn draw(cell: &RenderedCell, options: &RenderOptions) -> String {
    let width = options.width.get();
    let height = u16::try_from(cell.lines.len().max(1)).expect("the cell fits a screen");
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).expect("terminal opens");
    terminal
        .draw(|frame| {
            for (row, line) in (0..).zip(&cell.lines) {
                let area = ratatui::layout::Rect::new(0, row, width, 1);
                // A line widget with no text paints nothing, not even its own
                // style, so the fill goes on the buffer before the text.
                if let Some(fill) = line.fill {
                    let style = ratatui::style::Style::new().bg(ratatui_color(fill));
                    frame.buffer_mut().set_style(area, style);
                }
                frame.render_widget(ratatui_line(line), area);
            }
        })
        .expect("draws");
    format!(
        "{}\n{:?}",
        serde_json::to_string(options).expect("serializes"),
        terminal.backend().buffer()
    )
}

/// The text of the line as ratatui draws it.
fn ratatui_line(line: &StyledLine) -> ratatui::text::Line<'static> {
    let spans: Vec<_> = line
        .spans
        .iter()
        .map(|span| ratatui::text::Span::styled(span.text.clone(), ratatui_style(span.style)))
        .collect();
    ratatui::text::Line::from(spans)
}

fn ratatui_style(style: Style) -> ratatui::style::Style {
    use ratatui::style::Modifier;

    let mut drawn = ratatui::style::Style::new();
    if let Some(fg) = style.fg {
        drawn = drawn.fg(ratatui_color(fg));
    }
    if let Some(bg) = style.bg {
        drawn = drawn.bg(ratatui_color(bg));
    }
    let modifiers = [
        (style.bold, Modifier::BOLD),
        (style.dim, Modifier::DIM),
        (style.italic, Modifier::ITALIC),
        (style.underline, Modifier::UNDERLINED),
        (style.reverse, Modifier::REVERSED),
        (style.strikethrough, Modifier::CROSSED_OUT),
    ];
    for (on, modifier) in modifiers {
        if on {
            drawn = drawn.add_modifier(modifier);
        }
    }
    drawn
}

fn ratatui_color(color: Color) -> ratatui::style::Color {
    match color {
        Color::Indexed(index) => ratatui::style::Color::Indexed(index),
        Color::Rgb(red, green, blue) => ratatui::style::Color::Rgb(red, green, blue),
    }
}
