//! Display lines of the source of a cell.

use europa_core::{Color, RenderOptions, SourceLanguage, Style, StyledLine, StyledSpan};

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
/// background of the terminal, and nothing is read or written besides the
/// arguments.
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
/// Pass [`SourceLanguage::Plain`] for raw cells, and for code cells the value
/// of `Notebook::source_language` in `europa-core`.
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
                c if c.is_control() && c != '\t' => {
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
        .iter()
        .flat_map(|runs| wrap::wrap(runs, options.width))
        .map(|spans| StyledLine { spans, fill: None })
        .collect()
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

/// The source cut into pieces of one decoration each, by Python syntax.
fn python_runs(source: &str) -> Vec<(&str, Style)> {
    let mut runs = Vec::new();
    for (range, kind) in python::highlight(source) {
        // The ranges cover the source, so a range that does not fall on
        // characters means that the highlighter is wrong. Showing the source
        // whole is better than losing some of it.
        let Some(text) = source.get(range) else {
            return vec![(source, Style::default())];
        };
        runs.push((text, style(kind)));
    }
    runs
}

/// The decoration of a kind of token.
///
/// Only the first six colors of the palette are used, and no background, so
/// that the terminal's own theme decides how they look on dark and light
/// backgrounds alike.
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
        // Magic is only produced once IPython syntax is scanned for.
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
