//! Rendering backend for Europa.
//!
//! [`render_outputs`] turns the outputs of a code cell into styled display
//! lines for a terminal. The types of the result, such as [`RenderedCell`], are
//! defined in [`europa_core::render`], where every crate shares them.
//!
//! [`render_source`] does the same for the source of a cell, and highlights
//! Python by the kind of token.
//!
//! Lines from stderr, from errors and from code blocks carry a fill, which a
//! terminal paints to the right edge of the line, so that they stand out
//! from stdout.
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
mod html;
mod image;
mod ipython;
mod json;
mod markdown;
mod python;
mod source;
mod wrap;

pub use source::render_source;

use std::collections::VecDeque;
use std::num::NonZeroU16;

use europa_core::notebook::{ErrorOutput, UnknownOutput};
use europa_core::{
    resolve_carriage_returns, Background, Color, DecodedImage, ImageMime, ImageSlot, JsonMap,
    JsonValue, Output, RenderOptions, RenderedCell, Style, StyledLine, StyledSpan,
};
use unicode_width::UnicodeWidthStr;

use crate::image::ImageFailure;

// Palette entries rather than RGB, because some terminals cannot show 24-bit
// colors and neither ratatui nor crossterm reduces them.
const DARK_FILL: Color = Color::Indexed(52);
const LIGHT_FILL: Color = Color::Indexed(224);

// Greys rather than reds, so code blocks read apart from stderr and error
// outputs.
const DARK_CODE: Color = Color::Indexed(236);
const LIGHT_CODE: Color = Color::Indexed(254);

/// Background of code lines for the given terminal background.
pub(crate) fn code_bg(background: Background) -> Color {
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
    Blocks(Vec<blocks::Line>),
    /// Room for an image that was read and decoded.
    Image(PendingImage),
}

/// An image that waits for its place among the lines.
struct PendingImage {
    width: u16,
    height: u16,
    output: usize,
    mime: ImageMime,
    source: u64,
}

/// Render the outputs of a code cell as styled display lines.
///
/// The outputs are taken in order and go through these steps:
///
/// 1. Consecutive streams of one name, the chosen MIME of a display data or
///    execute result, and the traceback of an error each become a unit. The
///    choice is `text/markdown`, then `application/json`, then `text/plain`,
///    then `text/html`, skipping a `text/plain` that only names its object
///    while `text/html` is usable. Streams, tracebacks and `text/plain`
///    become a unit with a body. Markdown, JSON and HTML become indented
///    logical lines, with escape sequences and control characters removed
///    instead of read, and skip steps 2 and 3. When a non-blank entry gives
///    no line, `text/plain` that is not such a name, or a notice, stands in.
///    A blank entry gives no line. An output with nothing to draw,
///    such as an image or an unknown kind, becomes a notice that names what
///    it holds instead. A notice is wrapped like other text, so it can take
///    more than one line when it is wider than [`RenderOptions::width`].
///    A fenced code block of Markdown whose info string begins with a word
///    naming Python, such as `python`, `py` or `ipython`, is highlighted by
///    the rules of [`render_source`] once escape sequences and control
///    characters are removed.
/// 2. In the body of a unit, `\r` is resolved the way it is for a received
///    stream, so that a progress bar leaves only its last state. Notices skip
///    this step.
/// 3. Escape sequences and control characters are parsed into colors,
///    attributes and plain text, and the text is split into logical lines.
///    None of them reaches the result.
/// 4. Tabs are expanded and the logical lines are wrapped at
///    [`RenderOptions::width`] columns, counting grapheme clusters by their
///    width. The lines of stderr, of errors and of code blocks get a fill for
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
    render_outputs_with_images(outputs, options, &mut |_, _| {})
}

/// Like [`render_outputs`], and hands the decoded images to `sink`.
///
/// `sink` gets the `source` of each image and its pixels, in the order of the
/// outputs, including images that are left out because they are too far from
/// the ends of a long output. The result does not depend on `sink`. Decoding
/// takes time, so a caller should keep the result and not call this again
/// every frame.
#[must_use]
pub fn render_outputs_with_images(
    outputs: &[Output],
    options: &RenderOptions,
    sink: &mut dyn FnMut(u64, DecodedImage),
) -> RenderedCell {
    let fill = match options.background {
        Background::Dark => DARK_FILL,
        Background::Light => LIGHT_FILL,
    };
    let mut collector = Collector::new(options);
    for unit in units(outputs, options, sink) {
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
            Unit::Image(image) => collector.push_image(image),
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
    let (lines, images) = collector.finish();
    RenderedCell { lines, images }
}

/// Decodes the image that `slot` stands for in `outputs`.
///
/// Returns `None` when the outputs are no longer the ones the slot was made
/// from: the output is missing or holds no such image, or its data no longer
/// hashes to `slot.source`.
#[must_use]
pub fn decode_image(outputs: &[Output], slot: &ImageSlot) -> Option<DecodedImage> {
    let data = match outputs.get(slot.output)? {
        Output::DisplayData(display) => &display.data,
        Output::ExecuteResult(result) => &result.data,
        _ => return None,
    };
    let bytes = image::read_value(data.get(slot.mime.as_str())?).ok()?;
    if image::source_hash(&bytes) != slot.source {
        return None;
    }
    image::decode(&bytes).ok()
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
    /// Image slots and the line each starts at, counted over all lines.
    images: Vec<(usize, PendingImage)>,
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
            images: Vec::new(),
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

    /// Keeps room for an image: empty lines for a terminal to draw over.
    fn push_image(&mut self, image: PendingImage) {
        let height = image.height;
        self.images.push((self.total, image));
        for _ in 0..height {
            self.push(StyledLine::default());
        }
    }

    fn finish(self) -> (Vec<StyledLine>, Vec<ImageSlot>) {
        let slot = |line: usize, image: &PendingImage| ImageSlot {
            line,
            width: image.width,
            height: image.height,
            output: image.output,
            mime: image.mime,
            source: image.source,
        };
        if self.total <= self.max {
            let images = self
                .images
                .iter()
                .map(|(at, image)| slot(*at, image))
                .collect();
            return (self.head, images);
        }
        // A slot is kept whole or not at all, so a cut that falls inside one
        // moves to its edge.
        let mut head_len = (self.max - 1) / 2;
        let mut tail_start = self.total - self.tail_len;
        for (start, image) in &self.images {
            let end = start + usize::from(image.height);
            if *start < head_len && head_len < end {
                head_len = *start;
            }
            if *start < tail_start && tail_start < end {
                tail_start = end;
            }
        }
        let mut lines = self.head;
        lines.truncate(head_len);
        lines.push(notice(tail_start - head_len, self.width));
        let first_kept = self.total - self.tail.len();
        lines.extend(self.tail.into_iter().skip(tail_start - first_kept));
        let images = self
            .images
            .iter()
            .filter_map(|(start, image)| {
                if start + usize::from(image.height) <= head_len {
                    Some(slot(*start, image))
                } else if *start >= tail_start {
                    Some(slot(head_len + 1 + (start - tail_start), image))
                } else {
                    None
                }
            })
            .collect();
        (lines, images)
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
fn units(
    outputs: &[Output],
    options: &RenderOptions,
    sink: &mut dyn FnMut(u64, DecodedImage),
) -> Vec<Unit> {
    let mut units: Vec<Unit> = Vec::new();
    let mut stream: Option<&str> = None;
    for (index, output) in outputs.iter().enumerate() {
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
            Output::DisplayData(display) => units.push(bundle_unit(
                &display.data,
                &display.metadata,
                index,
                options,
                sink,
            )),
            Output::ExecuteResult(result) => units.push(bundle_unit(
                &result.data,
                &result.metadata,
                index,
                options,
                sink,
            )),
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

/// One MIME bundle reduced to the entry that is drawn.
enum Selected<'a> {
    /// Markdown drawn as logical lines.
    Markdown(String),
    /// JSON value drawn as indented lines.
    Json(&'a JsonValue),
    /// Plain text drawn through the text pipeline.
    Plain(String),
    /// HTML drawn as logical lines.
    Html(String),
    /// Nothing to draw, named by its types instead.
    None,
}

/// Choose the entry of a MIME bundle that is drawn.
fn select(data: &JsonMap) -> Selected<'_> {
    if let Some(text) = text_of(data, "text/markdown") {
        return Selected::Markdown(text);
    }
    if let Some(value) = data.get("application/json") {
        return Selected::Json(value);
    }
    if let Some(text) = text_of(data, "text/plain") {
        if is_default_repr(&text) {
            if let Some(html) = text_of(data, "text/html") {
                return Selected::Html(html);
            }
            return Selected::Plain(text);
        }
        return Selected::Plain(text);
    }
    if let Some(text) = text_of(data, "text/html") {
        return Selected::Html(text);
    }
    Selected::None
}

/// Whether plain text only names its object instead of showing it.
///
/// That is the repr of Python, `<module.Class object>`, or the one IPython
/// prints instead, `<module.Class at 0x7f...>`.
fn is_default_repr(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.contains('\n') || trimmed.contains('\r') || !trimmed.starts_with('<') {
        return false;
    }
    if trimmed.ends_with("object>") {
        return true;
    }
    trimmed
        .strip_suffix('>')
        .and_then(|rest| rest.rsplit_once(" at 0x"))
        .is_some_and(|(_, address)| {
            !address.is_empty() && address.chars().all(|c| c.is_ascii_hexdigit())
        })
}

/// The logical lines of Markdown, JSON or HTML in a MIME bundle, its plain
/// text as a body, or a notice that names its types when it has nothing to
/// show.
fn bundle_unit(
    data: &JsonMap,
    metadata: &JsonMap,
    output: usize,
    options: &RenderOptions,
    sink: &mut dyn FnMut(u64, DecodedImage),
) -> Unit {
    // An image that reads is shown whatever else the bundle holds. One that
    // does not read gives way to the next entry.
    let mut failures: Vec<(ImageMime, ImageFailure)> = Vec::new();
    for mime in [ImageMime::Png, ImageMime::Jpeg] {
        let Some(value) = data.get(mime.as_str()) else {
            continue;
        };
        let read = image::read_value(value).and_then(|bytes| {
            let decoded = image::decode(&bytes)?;
            Ok((bytes, decoded))
        });
        let (bytes, decoded) = match read {
            Ok(read) => read,
            Err(failure) => {
                failures.push((mime, failure));
                continue;
            }
        };
        let natural =
            image::natural_size(metadata.get(mime.as_str()), (decoded.width, decoded.height));
        let (width, height) = image::slot_size(natural, options);
        let source = image::source_hash(&bytes);
        sink(source, decoded);
        return Unit::Image(PendingImage {
            width,
            height,
            output,
            mime,
            source,
        });
    }
    let (lines, blank) = match select(data) {
        Selected::Markdown(text) => (
            markdown::render(&text, options.width, options.background),
            text.trim().is_empty(),
        ),
        Selected::Json(value) => (json::render(value), false),
        Selected::Plain(text) => return Unit::Body { text, fill: false },
        Selected::Html(text) => (
            html::render(&text, options.width, options.background),
            text.trim().is_empty(),
        ),
        Selected::None => return types_notice(data, &failures),
    };
    // An empty entry shows nothing on purpose.
    if !lines.is_empty() || blank {
        return Unit::Blocks(lines);
    }
    // A rich entry whose content renders to nothing, such as HTML that only
    // holds a script, would leave no trace, so plain text that shows the
    // object or the types stand in.
    match text_of(data, "text/plain") {
        Some(text) if !is_default_repr(&text) => Unit::Body { text, fill: false },
        _ => types_notice(data, &failures),
    }
}

/// The notice that names the types of a MIME bundle.
///
/// A PNG or JPEG entry that was refused is followed by the reason.
fn types_notice(data: &JsonMap, failures: &[(ImageMime, ImageFailure)]) -> Unit {
    if data.is_empty() {
        return Unit::Notice("[no data]".to_owned());
    }
    let types: Vec<String> = data
        .keys()
        .map(|name| {
            let failure = failures
                .iter()
                .find(|(mime, _)| mime.as_str() == name)
                .map(|(_, failure)| failure);
            match failure {
                Some(ImageFailure::Broken) => format!("{name}: broken"),
                Some(ImageFailure::TooLarge) => format!("{name}: too large"),
                None => name.clone(),
            }
        })
        .collect();
    Unit::Notice(format!("[{}]", types.join(", ")))
}

fn unknown_notice(unknown: &UnknownOutput) -> String {
    match unknown.value.get("output_type") {
        Some(JsonValue::String(kind)) => format!("[unknown output: {kind}]"),
        _ => "[unknown output]".to_owned(),
    }
}

/// The text of one MIME entry, if it is a string or an array of strings.
fn text_of(data: &JsonMap, mime: &str) -> Option<String> {
    match data.get(mime)? {
        JsonValue::String(text) => Some(text.clone()),
        JsonValue::Array(parts) => parts.iter().map(JsonValue::as_str).collect(),
        _ => None,
    }
}
