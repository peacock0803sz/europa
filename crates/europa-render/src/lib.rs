//! Rendering backend for Europa.
//!
//! [`render_outputs`] turns the outputs of a code cell into styled display
//! lines for a terminal. The types of the result, such as [`RenderedCell`], are
//! defined in [`europa_core::render`], where every crate shares them.
#![deny(missing_docs)]

mod ansi;
mod wrap;

use std::collections::VecDeque;
use std::num::NonZeroU16;

use europa_core::notebook::ErrorOutput;
use europa_core::{
    resolve_carriage_returns, Background, Color, JsonMap, JsonValue, Output, RenderOptions,
    RenderedCell, Style, StyledLine, StyledSpan,
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
    let mut collector = Collector::new(options);
    for unit in units(outputs) {
        // The rule runs on the text before escape sequences are read, so a
        // color set earlier on a line is overwritten with the line, the way
        // it is when the text arrives live.
        for runs in ansi::parse(&resolve_carriage_returns(&unit.text)) {
            for spans in wrap::wrap(&runs, options.width) {
                collector.push(StyledLine {
                    spans,
                    fill: unit.fill.then_some(fill),
                });
            }
        }
    }
    RenderedCell {
        lines: collector.finish(),
        images: Vec::new(),
    }
}

/// Collects the display lines of a cell, and drops the middle of a long output.
///
/// Lines are counted as they arrive instead of being all held, so the memory
/// for display lines stays in proportion to the limit however long the output
/// is.
struct Collector {
    max: usize,
    width: NonZeroU16,
    /// The first `max` lines, which are all of them when the output fits.
    head: Vec<StyledLine>,
    /// The last lines, as many as are shown after the notice.
    tail: VecDeque<StyledLine>,
    tail_len: usize,
    total: usize,
}

impl Collector {
    fn new(options: &RenderOptions) -> Self {
        let max = options.max_output_lines.get();
        Self {
            max,
            width: options.width,
            head: Vec::new(),
            tail: VecDeque::new(),
            // The notice takes a line, and of the rest the start gets the
            // smaller half, so the end gets the extra line when it is odd.
            tail_len: max - 1 - (max - 1) / 2,
            total: 0,
        }
    }

    fn push(&mut self, line: StyledLine) {
        self.total += 1;
        if self.head.len() < self.max {
            self.head.push(line.clone());
        }
        self.tail.push_back(line);
        if self.tail.len() > self.tail_len {
            self.tail.pop_front();
        }
    }

    fn finish(self) -> Vec<StyledLine> {
        if self.total <= self.max {
            return self.head;
        }
        let mut lines = self.head;
        lines.truncate((self.max - 1) / 2);
        lines.push(notice(self.total - self.max + 1, self.width));
        lines.extend(self.tail);
        lines
    }
}

/// The line that stands for the `hidden` lines left out of a long output.
///
/// It is cut to the width instead of wrapped, so that it is always one line
/// and the cell stays within its limit.
fn notice(hidden: usize, width: NonZeroU16) -> StyledLine {
    StyledLine {
        spans: vec![StyledSpan {
            text: wrap::cut(&format!("... {hidden} lines hidden ..."), width),
            style: Style {
                dim: true,
                ..Style::default()
            },
        }],
        fill: None,
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
