//! Drawing a rendered cell: its lines, and its images in their slots.

use europa_core::{Output, RenderedCell};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::image_view::{ImageRenderer, Placement};

/// Draws `cell` in `area`, from the line `scroll` of the cell.
///
/// The lines go first, and then the images over the empty lines of their
/// slots. `area` is as wide as the width the cell was rendered for, and does
/// not include the last line of the terminal, because a Sixel image drawn
/// there makes the terminal scroll.
pub(crate) fn draw_cell(
    buf: &mut Buffer,
    area: Rect,
    cell: &RenderedCell,
    outputs: &[Output],
    scroll: usize,
    renderer: &mut ImageRenderer,
) {
    todo!()
}

/// Where the images of `cell` are on the screen, for the slots that are
/// visible in `area` when the cell is drawn from the line `scroll`.
pub(crate) fn visible_placements(area: Rect, cell: &RenderedCell, scroll: usize) -> Vec<Placement> {
    todo!()
}
