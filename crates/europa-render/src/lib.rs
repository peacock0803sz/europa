//! Rendering backend for Europa.
//!
//! [`render_outputs`] turns the outputs of a code cell into styled display
//! lines for a terminal. The types of the result, such as [`RenderedCell`], are
//! defined in [`europa_core::render`], where every crate shares them.
//!
//! Lines from stderr and from errors carry a fill, which a terminal paints to
//! the right edge of the line, so that they stand out from stdout.
//!
//! ```
//! use std::num::NonZeroU16;
//!
//! use europa_core::{Output, RenderOptions};
//! use europa_render::render_outputs;
//!
//! let outputs = [
//!     Output::new_stream("stdout", "ok\n"),
//!     Output::new_stream("stderr", "\u{1b}[1mwarning\u{1b}[0m\n"),
//! ];
//! let options = RenderOptions::new(NonZeroU16::new(20).expect("20 is not zero"));
//! let cell = render_outputs(&outputs, &options);
//!
//! let lines: Vec<String> = cell
//!     .lines
//!     .iter()
//!     .map(|line| line.spans.iter().map(|span| span.text.as_str()).collect())
//!     .collect();
//! assert_eq!(lines, ["ok", "warning"]);
//! assert_eq!(cell.lines[0].fill, None);
//! assert!(cell.lines[1].fill.is_some());
//! ```
#![deny(missing_docs)]

mod ansi;
mod blocks;
mod wrap;

use std::collections::VecDeque;
use std::num::NonZeroU16;

use europa_core::notebook::{ErrorOutput, UnknownOutput};
use europa_core::{
    resolve_carriage_returns, Background, Color, JsonMap, JsonValue, Output, RenderOptions,
    RenderedCell, Style, StyledLine, StyledSpan,
};
use unicode_width::UnicodeWidthStr;

// Palette entries rather than RGB, because some terminals cannot show 24-bit
// colors and neither ratatui nor crossterm reduces them.
const DARK_FILL: Color = Color::Indexed(52);
const LIGHT_FILL: Color = Color::Indexed(224);

// Grey shades with a hue kept away from the red fills, so code blocks read
// apart from stderr and error outputs.
#[allow(dead_code)]
const DARK_CODE: Color = Color::Indexed(236);
#[allow(dead_code)]
const LIGHT_CODE: Color = Color::Indexed(254);

/// Background of code lines for the given terminal background.
#[allow(dead_code)]
fn code_bg(background: Background) -> Color {
    match background {
        Background::Dark => DARK_CODE,
        Background::Light => LIGHT_CODE,
    }
}

/// Text that is rendered as a whole.
enum Unit {
    /// Text of an output, which can hold escape sequences and carriage returns.
    Body {
        text: String,
        /// Whether the lines get the fill that marks stderr.
        fill: bool,
    },
    /// Text that stands for an output that cannot be shown.
    Notice(String),
    /// Logical lines from a rich output, already styled and indented.
    #[allow(dead_code)]
    Blocks(Vec<blocks::Line>),
}

/// Render the outputs of a code cell as styled display lines.
///
/// The outputs are taken in order and go through these steps:
///
/// 1. Consecutive streams of one name, the `text/plain` of a display data or
///    execute result, and the traceback of an error each become a unit with
///    a body. An output with no text to show, such as an image or an unknown
///    kind, becomes a notice that names what it holds instead. A notice is
///    wrapped like other text, so it can take more than one line when it is
///    wider than [`RenderOptions::width`].
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
        match unit {
            Unit::Blocks(lines) => {
                for line in &lines {
                    for styled in wrap_line(line, options.width) {
                        collector.push(styled);
                    }
                }
            }
            // The rule runs on the text before escape sequences are read, so a
            // color set earlier on a line is overwritten with the line, the
            // way it is when the text arrives live.
            Unit::Body { text, fill: shaded } => {
                let fill = shaded.then_some(fill);
                for runs in ansi::parse(&resolve_carriage_returns(&text)) {
                    for spans in wrap::wrap(&runs, options.width) {
                        collector.push(StyledLine { spans, fill });
                    }
                }
            }
            // The names in a notice come from the file and can hold escape
            // sequences, so a notice is read like any other text.
            Unit::Notice(text) => {
                for runs in ansi::parse(&text).into_iter().map(dimmed) {
                    for spans in wrap::wrap(&runs, options.width) {
                        collector.push(StyledLine { spans, fill: None });
                    }
                }
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
/// Lines are counted as they arrive instead of being all held, so the lines
/// it keeps stay in proportion to the limit however long the output is. The
/// lines of one unit are still all built before they reach it.
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
            style: dim(),
        }],
        fill: None,
    }
}

/// The style of notices. Dim tells a notice from output that reads the same,
/// and italic would not do, because tmux can draw it as reverse.
fn dim() -> Style {
    Style {
        dim: true,
        ..Style::default()
    }
}

fn dimmed(runs: Vec<StyledSpan>) -> Vec<StyledSpan> {
    runs.into_iter()
        .map(|run| StyledSpan {
            style: dim(),
            ..run
        })
        .collect()
}

/// Break one logical line into display lines with its indentation.
fn wrap_line(line: &blocks::Line, width: NonZeroU16) -> Vec<StyledLine> {
    let indent: usize = line
        .lead
        .iter()
        .map(|span| UnicodeWidthStr::width(span.text.as_str()))
        .sum();
    if line.spans.is_empty() {
        return vec![StyledLine {
            spans: normalize(line.lead.clone()),
            fill: line.fill,
        }];
    }
    if indent < usize::from(width.get()) {
        let inner = NonZeroU16::new((usize::from(width.get()) - indent) as u16)
            .expect("indent leaves at least one column");
        wrap::wrap(&line.spans, inner)
            .into_iter()
            .enumerate()
            .map(|(index, body)| {
                let mut spans = if index == 0 {
                    line.lead.clone()
                } else {
                    line.hang.clone()
                };
                spans.extend(body);
                StyledLine {
                    spans: normalize(spans),
                    fill: line.fill,
                }
            })
            .collect()
    } else {
        let mut joined = line.lead.clone();
        joined.extend(line.spans.clone());
        wrap::wrap(&joined, width)
            .into_iter()
            .map(|spans| StyledLine {
                spans: normalize(spans),
                fill: line.fill,
            })
            .collect()
    }
}

/// Drop empty spans and join neighbours with the same decoration.
fn normalize(spans: Vec<StyledSpan>) -> Vec<StyledSpan> {
    let mut kept: Vec<StyledSpan> = Vec::new();
    for span in spans {
        if span.text.is_empty() {
            continue;
        }
        match kept.last_mut() {
            Some(last) if last.style == span.style => last.text.push_str(&span.text),
            _ => kept.push(span),
        }
    }
    kept
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
                    Some(Unit::Body { text: last, .. })
                        if previous == Some(output.name.as_str()) =>
                    {
                        last.push_str(text);
                    }
                    _ => units.push(Unit::Body {
                        text: text.to_owned(),
                        fill: output.name == "stderr",
                    }),
                }
                stream = Some(&output.name);
            }
            Output::DisplayData(display) => units.push(bundle_unit(&display.data)),
            Output::ExecuteResult(result) => units.push(bundle_unit(&result.data)),
            Output::Error(error) => units.push(Unit::Body {
                text: traceback_text(error),
                fill: true,
            }),
            Output::Unknown(unknown) => units.push(Unit::Notice(unknown_notice(unknown))),
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

/// The text of a MIME bundle, or a notice that names its types when it has no
/// text to show.
fn bundle_unit(data: &JsonMap) -> Unit {
    match plain_text(data) {
        Some(text) => Unit::Body { text, fill: false },
        None if data.is_empty() => Unit::Notice("[no data]".to_owned()),
        None => {
            let types: Vec<&str> = data.keys().map(String::as_str).collect();
            Unit::Notice(format!("[{}]", types.join(", ")))
        }
    }
}

fn unknown_notice(unknown: &UnknownOutput) -> String {
    match unknown.value.get("output_type") {
        Some(JsonValue::String(kind)) => format!("[unknown output: {kind}]"),
        _ => "[unknown output]".to_owned(),
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
