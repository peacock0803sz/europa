//! Display lines of one cell: border, source, separator and outputs.

use europa_core::{Cell, DecodedImage, RenderOptions, RenderedCell, SourceLanguage};

/// Render one cell as border, source, separator and output lines.
pub(crate) fn cell_display(
    cell: &Cell,
    number: usize,
    language: SourceLanguage,
    options: &RenderOptions,
    sink: &mut dyn FnMut(u64, DecodedImage),
) -> RenderedCell {
    let _ = (cell, number, language, options, sink);
    todo!()
}
