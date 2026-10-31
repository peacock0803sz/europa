//! Results of rendering a cell's outputs, independent of any terminal.
//!
//! `europa-render` builds these types and `europa-tui` draws them. They live
//! here, with the other types the crates share, so that neither crate needs
//! the other's types to talk about a rendered cell.

use std::num::{NonZeroU16, NonZeroUsize};

use serde::{Deserialize, Serialize};

/// A terminal color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Color {
    /// A color of the 256-color palette. 0 to 15 are the 16 standard colors
    /// that SGR names.
    Indexed(u8),
    /// A 24-bit color given as red, green and blue.
    Rgb(u8, u8, u8),
}

/// Colors and attributes of a run of characters.
///
/// The default is plain text in the terminal's own colors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    /// Foreground color. `None` is the terminal's default color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg: Option<Color>,
    /// Background color. `None` is the terminal's default color, and the
    /// [`StyledLine::fill`] of the line shows instead where the line has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<Color>,
    /// Bold text.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    /// Dim text.
    #[serde(default, skip_serializing_if = "is_false")]
    pub dim: bool,
    /// Italic text.
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    /// Underlined text.
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    /// Foreground and background swapped.
    #[serde(default, skip_serializing_if = "is_false")]
    pub reverse: bool,
    /// Text with a line through it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub strikethrough: bool,
}

/// serde calls skip predicates with a reference.
fn is_false(value: &bool) -> bool {
    !*value
}

/// A run of characters that share one [`Style`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyledSpan {
    /// The characters. Never empty, and free of escape sequences and control
    /// characters.
    pub text: String,
    /// How the characters are drawn.
    #[serde(default)]
    pub style: Style,
}

/// One display line, as spans from left to right.
///
/// Adjacent spans differ in style. The widths of the spans add up to at most
/// the width the line was rendered for. The one exception is a line holding
/// a single grapheme cluster that is wider than that.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyledLine {
    /// The spans of the line. Empty for a line without visible characters.
    #[serde(default)]
    pub spans: Vec<StyledSpan>,
    /// Background color to paint from the end of the text to the right edge
    /// of the line. Only the lines of stderr and of error outputs have one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
}

/// Room that a rendered cell keeps for an image.
///
/// Images are not rendered yet, so [`RenderedCell::images`] stays empty, and
/// the fields may change when they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageSlot {
    /// Index in [`RenderedCell::lines`] of the line where the image starts.
    pub line: usize,
    /// Columns to keep.
    pub width: u16,
    /// Lines to keep.
    pub height: u16,
}

/// The display lines of one cell's outputs, ready for a terminal to draw.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedCell {
    /// Display lines from top to bottom. There are at most
    /// [`RenderOptions::max_output_lines`] of them.
    #[serde(default)]
    pub lines: Vec<StyledLine>,
    /// Room for images. Always empty until images are rendered.
    #[serde(default)]
    pub images: Vec<ImageSlot>,
}

/// Whether the terminal background is dark or light.
///
/// Colors that sit behind text, such as the fill of stderr lines, depend on
/// it so that the text stays readable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    /// A dark background. This is the default.
    #[default]
    Dark,
    /// A light background.
    Light,
}

/// What a rendering depends on besides the outputs themselves.
///
/// Zero is not a valid width or limit, and the types say so: reading a zero
/// from serde fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderOptions {
    /// Width of the area to draw in, in columns.
    pub width: NonZeroU16,
    /// Most display lines a rendered cell may have. A longer output keeps its
    /// beginning and end and drops the middle.
    #[serde(default = "default_max_output_lines")]
    pub max_output_lines: NonZeroUsize,
    /// The background that colors are chosen for.
    #[serde(default)]
    pub background: Background,
}

/// serde needs a function for a default that is not a `Default` value.
fn default_max_output_lines() -> NonZeroUsize {
    RenderOptions::DEFAULT_MAX_OUTPUT_LINES
}

impl RenderOptions {
    /// The default for [`RenderOptions::max_output_lines`].
    pub const DEFAULT_MAX_OUTPUT_LINES: NonZeroUsize =
        NonZeroUsize::new(200).expect("200 is not zero");

    /// Options for an area `width` columns wide, with the default line limit
    /// and a dark background.
    #[must_use]
    pub fn new(width: NonZeroU16) -> Self {
        Self {
            width,
            max_output_lines: Self::DEFAULT_MAX_OUTPUT_LINES,
            background: Background::Dark,
        }
    }
}
