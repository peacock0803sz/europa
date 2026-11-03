//! Display lines of the source of a cell.

use std::ops::Range;

use europa_core::{Color, RenderOptions, SourceLanguage, Style, StyledLine, StyledSpan};

use crate::ipython;
use crate::python::{self, Kind};
use crate::wrap;

/// Decoration of the pictures that stand for control characters.
const CONTROL: Style = Style {
    fg: Some(Color::Indexed(1)),
    bg: None,
    bold: true,
    dim: false,
    italic: false,
    underline: false,
    reverse: false,
    strikethrough: false,
};

/// Renders the source of a cell as display lines.
///
/// Unlike the outputs of a cell, the source is never cut at a limit of lines,
/// and its lines have no `fill`. The decoration does not depend on the
/// background of the terminal, and the result depends only on the arguments.
///
/// The steps are:
///
/// 1. `\r\n` becomes `\n`.
/// 2. The source is divided into ranges of one decoration each.
/// 3. Control characters other than the line feed and the tab become pictures
///    such as `␛`.
/// 4. The source is cut into logical lines at `\n`. No logical line follows a
///    final `\n`.
/// 5. Each logical line is wrapped to the width and its tabs are expanded.
///
/// Python is decorated by the kind of token: keywords, built-in constants,
/// strings, escapes, comments, numbers, the names given at definitions,
/// calls of built-in functions, decorators and the syntax of f-strings. The
/// colors are 1 to 6 of the 16-color palette, red to cyan, so the theme of the
/// terminal decides how they look. Comments are dim without a color, and some
/// kinds are bold. A replacement field of an f-string drops the color of the
/// string around it, and the tokens inside are decorated as Python again, so
/// the `1` in `f"total: {n + 1}"` is a number. Control pictures are bold in
/// color 1. No background is used.
///
/// IPython syntax is recognized in Python source. A line magic or shell escape
/// that starts a logical line, or follows the first `=` of one, is decorated
/// as a magic from its `%` or `!` to the end of the line, and so is the first
/// line of a cell magic such as `%%timeit`. The lines after `%%time`,
/// `%%timeit`, `%%capture` and `%%prun` are Python; after any other cell magic
/// they are left undecorated. A `%` or `!` inside a string, a bracket or a line
/// continuation is not a magic.
///
/// Pass [`SourceLanguage::Plain`] for raw cells, and for code cells the value
/// of [`europa_core::Notebook::source_language`].
///
/// ```
/// use std::num::NonZeroU16;
///
/// use europa_core::{RenderOptions, SourceLanguage};
/// use europa_render::render_source;
///
/// let options = RenderOptions::new(NonZeroU16::new(80).unwrap());
/// assert!(render_source("", SourceLanguage::Plain, &options).is_empty());
/// assert_eq!(render_source("a\n\n", SourceLanguage::Plain, &options).len(), 2);
/// ```
#[must_use]
pub fn render_source(
    source: &str,
    language: SourceLanguage,
    options: &RenderOptions,
) -> Vec<StyledLine> {
    logical_lines(source, language, true)
        .iter()
        .flat_map(|runs| wrap::wrap(runs, options.width))
        .map(|spans| StyledLine { spans, fill: None })
        .collect()
}

/// The source as Python split into logical lines with a decoration on each
/// span, before wrapping.
///
/// Control characters are left as they are. The text of Markdown has them
/// removed before it comes here, so the pictures of `render_source` are not
/// wanted.
pub(crate) fn highlight_lines(source: &str) -> Vec<Vec<StyledSpan>> {
    logical_lines(source, SourceLanguage::Python, false)
}

/// Cuts the source into logical lines of decorated spans.
///
/// With `pictures`, control characters other than the line feed and the tab
/// become pictures that stand for them.
fn logical_lines(source: &str, language: SourceLanguage, pictures: bool) -> Vec<Vec<StyledSpan>> {
    let source = source.replace("\r\n", "\n");
    let styled = match language {
        SourceLanguage::Python => python_runs(&source),
        SourceLanguage::Plain => vec![(source.as_str(), Style::default())],
    };

    let mut logical: Vec<Vec<StyledSpan>> = Vec::new();
    let mut current: Vec<StyledSpan> = Vec::new();
    for (text, style) in styled {
        for c in text.chars() {
            match c {
                '\n' => logical.push(std::mem::take(&mut current)),
                c if pictures && c.is_control() && c != '\t' => {
                    push_char(&mut current, control_picture(c), CONTROL);
                }
                c => push_char(&mut current, c, style),
            }
        }
    }
    // A final line feed ends the last line, and does not start another.
    if !source.is_empty() && !source.ends_with('\n') {
        logical.push(current);
    }
    logical
}

/// Adds `c` to the line, joining it to the last span if the style is the same.
fn push_char(line: &mut Vec<StyledSpan>, c: char, style: Style) {
    match line.last_mut() {
        Some(last) if last.style == style => last.text.push(c),
        _ => line.push(StyledSpan {
            text: c.to_string(),
            style,
        }),
    }
}

/// The picture that stands for the control character `c`.
///
/// C0 characters go to the Control Pictures block at the same offset, DEL has
/// a picture of its own and C1 characters, which the block has none for, share
/// the picture for a substitute.
fn control_picture(c: char) -> char {
    match c {
        '\u{7f}' => '\u{2421}',
        c if (c as u32) < 0x20 => char::from_u32(0x2400 + c as u32).unwrap_or('\u{2426}'),
        _ => '\u{2426}',
    }
}

/// The source cut into pieces of one decoration each, by Python syntax and
/// IPython magics.
///
/// Only the part of the source that is Python is highlighted, with its magics
/// masked, and then the magics are decorated on top. What is neither Python
/// nor a magic, such as the body of `%%bash`, has no decoration.
fn python_runs(source: &str) -> Vec<(&str, Style)> {
    let magics = ipython::scan(source);
    let python = magics.python.clone();

    let mut cuts: Vec<(Range<usize>, Style)> = Vec::new();
    cuts.push((0..python.start, Style::default()));
    for (range, kind) in python::highlight(&magics.masked[python.clone()]) {
        cuts.push((
            range.start + python.start..range.end + python.start,
            style(kind),
        ));
    }
    cuts.push((python.end..source.len(), Style::default()));

    let mut runs: Vec<(Range<usize>, Style)> = Vec::new();
    let mut next_magic = 0;
    for (range, base) in cuts {
        let mut at = range.start;
        while at < range.end {
            while next_magic < magics.ranges.len() && magics.ranges[next_magic].end <= at {
                next_magic += 1;
            }
            let (end, decoration) = match magics.ranges.get(next_magic) {
                Some(magic) if magic.start <= at => {
                    (magic.end.min(range.end), style(Some(Kind::Magic)))
                }
                Some(magic) if magic.start < range.end => (magic.start, base),
                _ => (range.end, base),
            };
            match runs.last_mut() {
                Some((last, last_style)) if *last_style == decoration && last.end == at => {
                    last.end = end;
                }
                _ => runs.push((at..end, decoration)),
            }
            at = end;
        }
    }

    let mut pieces = Vec::new();
    for (range, style) in runs {
        // The ranges cover the source, so a range that does not fall on
        // characters means that a range is wrong. Showing the source whole is
        // better than losing some of it.
        let Some(text) = source.get(range) else {
            return vec![(source, Style::default())];
        };
        pieces.push((text, style));
    }
    pieces
}

/// The decoration of a kind of token.
///
/// Only colors 1 to 6 of the palette are used, with bold or dim, and no
/// background, so that the terminal's own theme decides how they look on dark
/// and light backgrounds alike.
fn style(kind: Option<Kind>) -> Style {
    let (index, bold) = match kind {
        Some(Kind::Keyword) => (5, false),
        Some(Kind::ConstantBuiltin) => (5, true),
        Some(Kind::String) => (2, false),
        Some(Kind::StringEscape) => (2, true),
        Some(Kind::Number) => (6, false),
        Some(Kind::FunctionDefinition) => (4, true),
        Some(Kind::FunctionBuiltin) => (4, false),
        Some(Kind::Decorator) => (3, true),
        Some(Kind::FStringSyntax) => (3, false),
        // IPython syntax found by the scan in `ipython`.
        Some(Kind::Magic) => (6, true),
        Some(Kind::Comment) => {
            return Style {
                dim: true,
                ..Style::default()
            }
        }
        // An embedded expression is Python again, so it must not carry the
        // decoration of the string around it.
        Some(Kind::Embedded) | None => return Style::default(),
    };
    Style {
        fg: Some(Color::Indexed(index)),
        bold,
        ..Style::default()
    }
}
