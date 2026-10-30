//! Rendering backend for Europa.
//!
//! [`render_outputs`] turns the outputs of a code cell into styled display
//! lines for a terminal. The types of the result, such as [`RenderedCell`], are
//! defined in [`europa_core::render`], where every crate shares them.
#![deny(missing_docs)]

use europa_core::{Output, RenderOptions, RenderedCell};

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
    let _ = (outputs, options);
    todo!()
}
