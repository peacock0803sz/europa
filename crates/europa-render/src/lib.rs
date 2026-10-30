//! Rendering backend for Europa.
//!
//! [`render_outputs`] turns the outputs of a code cell into styled display
//! lines for a terminal. The types of the result, such as [`RenderedCell`], are
//! defined in [`europa_core::render`], where every crate shares them.
#![deny(missing_docs)]

mod ansi;
mod wrap;

use europa_core::notebook::ErrorOutput;
use europa_core::{
    Background, Color, JsonMap, JsonValue, Output, RenderOptions, RenderedCell, StyledLine,
};

// Palette entries rather than RGB, because some terminals cannot show 24-bit
// colors and neither ratatui nor crossterm reduces them.
const DARK_FILL: Color = Color::Indexed(52);
const LIGHT_FILL: Color = Color::Indexed(224);

/// Text that is rendered as a whole.
struct Unit {
    text: String,
    /// Whether the lines get the fill that marks stderr.
    fill: bool,
}

/// Render the outputs of a code cell as styled display lines.
///
/// The outputs are taken in order and go through these steps:
///
/// 1. Consecutive streams of one name, the `text/plain` of a display data or
///    execute result, and the traceback of an error each become a unit with
///    a body. An output with no text to show, such as an image or an unknown
///    kind, becomes a one-line notice instead.
/// 2. In the body of a unit, `\r` is resolved the way it is for a received
///    stream, so that a progress bar leaves only its last state. Notices skip
///    this step.
/// 3. Escape sequences and control characters are parsed into colors,
///    attributes and plain text, and the text is split into logical lines.
///    None of them reaches the result.
/// 4. Tabs are expanded and the logical lines are wrapped at
///    [`RenderOptions::width`] columns, counting grapheme clusters by their
///    width. The lines of stderr and of errors get a fill for
///    [`RenderOptions::background`].
/// 5. When there are more lines than [`RenderOptions::max_output_lines`], the
///    beginning and the end are kept and one line that counts the hidden
///    lines replaces the middle.
/// 6. The lines are returned without image slots.
///
/// The function does no IO and reads nothing but its arguments, so equal
/// arguments give equal results. It never changes `outputs`. The result has at
/// most [`RenderOptions::max_output_lines`] lines, and its `images` is empty
/// for now.
#[must_use]
pub fn render_outputs(outputs: &[Output], options: &RenderOptions) -> RenderedCell {
    let fill = match options.background {
        Background::Dark => DARK_FILL,
        Background::Light => LIGHT_FILL,
    };
    let mut lines = Vec::new();
    for unit in units(outputs) {
        for runs in ansi::parse(&unit.text) {
            for spans in wrap::wrap(&runs, options.width) {
                lines.push(StyledLine {
                    spans,
                    fill: unit.fill.then_some(fill),
                });
            }
        }
    }
    RenderedCell {
        lines,
        images: Vec::new(),
    }
}

/// Group the outputs into the units that are rendered as a whole.
fn units(outputs: &[Output]) -> Vec<Unit> {
    let mut units: Vec<Unit> = Vec::new();
    let mut stream: Option<&str> = None;
    for output in outputs {
        // The name of the stream that the last unit holds, if the last output
        // was a stream.
        let previous = stream.take();
        match output {
            Output::Stream(output) => {
                let text = output.text.text();
                match units.last_mut() {
                    Some(last) if previous == Some(output.name.as_str()) => {
                        last.text.push_str(text);
                    }
                    _ => units.push(Unit {
                        text: text.to_owned(),
                        fill: output.name == "stderr",
                    }),
                }
                stream = Some(&output.name);
            }
            Output::DisplayData(display) => units.push(plain_unit(&display.data)),
            Output::ExecuteResult(result) => units.push(plain_unit(&result.data)),
            Output::Error(error) => units.push(Unit {
                text: traceback_text(error),
                fill: true,
            }),
            Output::Unknown(_) => todo!(),
        }
    }
    units
}

/// The entries of a traceback joined with newlines, as IPython prints them,
/// or the name and value of the exception when there is nothing to print.
///
/// The entries are read as one text, so a color set in one carries over to the
/// next, and an entry that ends in a newline is followed by an empty line.
fn traceback_text(error: &ErrorOutput) -> String {
    if error.traceback.iter().all(String::is_empty) {
        format!("{}: {}", error.ename, error.evalue)
    } else {
        error.traceback.join("\n")
    }
}

fn plain_unit(data: &JsonMap) -> Unit {
    match plain_text(data) {
        Some(text) => Unit { text, fill: false },
        None => todo!(),
    }
}

/// The `text/plain` of a MIME bundle, if it is a string or an array of strings.
fn plain_text(data: &JsonMap) -> Option<String> {
    match data.get("text/plain")? {
        JsonValue::String(text) => Some(text.clone()),
        JsonValue::Array(parts) => parts.iter().map(JsonValue::as_str).collect(),
        _ => None,
    }
}
