//! Frame drawing of the viewer.

use europa_core::Notebook;
use ratatui::buffer::Buffer;

use crate::image_view::{ImageRenderer, Placement};
use crate::viewer::Viewer;

/// Draw the visible cells and the status line.
pub(crate) fn view(
    viewer: &Viewer,
    notebook: &Notebook,
    buf: &mut Buffer,
    renderer: &mut ImageRenderer,
) -> Vec<Placement> {
    let _ = (viewer, notebook, buf, renderer);
    todo!()
}
