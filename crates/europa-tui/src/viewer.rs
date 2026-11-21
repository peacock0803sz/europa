//! Viewer state and its transitions.

use std::collections::BTreeMap;

use europa_core::{Notebook, RenderOptions, RenderedCell, SourceLanguage};

use crate::image_view::ImageRenderer;

/// Where the top of the viewport is.
///
/// `row` is the line within the cell, with 0 being the border line of the
/// cell, and is smaller than the height of the cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Position {
    /// Index of the cell at the top, counting from 0.
    pub(crate) cell: usize,
    /// Line within that cell, counting from its border line.
    pub(crate) row: usize,
}

/// What a key press means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    /// Quit the viewer.
    Quit,
    /// Any other key.
    Other,
}

/// Input given to the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Input {
    /// A key was pressed.
    Key(Key),
    /// The terminal changed size.
    Resize {
        /// New width in columns.
        columns: u16,
        /// New height in lines.
        rows: u16,
        /// New size of a cell in pixels.
        cell_pixels: europa_core::CellPixels,
    },
}

/// What the viewer asks the event loop to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Effect {
    /// Keep going.
    None,
    /// Quit with exit code 0.
    Quit,
}

/// Makes display lines for one cell.
pub(crate) trait CellRenderer {
    /// Render the cell `index` with `options`.
    fn render(&mut self, index: usize, options: &RenderOptions) -> RenderedCell;
}

/// What the viewer shows.
pub(crate) struct Viewer {
    /// File name shown in the status line.
    pub(crate) file_name: String,
    /// Number of cells in the notebook.
    pub(crate) cell_count: usize,
    /// Where the top of the viewport is.
    pub(crate) position: Position,
    /// Width the cells are rendered for.
    pub(crate) columns: u16,
    /// Height of the terminal in lines.
    pub(crate) rows: u16,
    /// How cells are rendered.
    pub(crate) options: RenderOptions,
    /// Rendered cells that are visible.
    ///
    /// The keys are consecutive numbers starting at `position.cell`, ending
    /// where their heights reach the viewport lines or at the last cell. It
    /// is empty when the viewport has no lines, the width is zero, or there
    /// are no cells.
    ///
    /// The position stays between the first display position and the bottom
    /// limit. When visible is not empty, `position.row` is smaller than the
    /// lines of `visible[&position.cell]`.
    ///
    /// The background is decided when opening and stays the same until the
    /// viewer closes.
    pub(crate) visible: BTreeMap<usize, RenderedCell>,
}

impl Viewer {
    /// Open a notebook at its first display position, rendering only visible cells.
    pub(crate) fn open(
        file_name: String,
        cell_count: usize,
        columns: u16,
        rows: u16,
        options: RenderOptions,
        renderer: &mut dyn CellRenderer,
    ) -> Viewer {
        let _ = (file_name, cell_count, columns, rows, options, renderer);
        todo!()
    }

    /// First and last cells with at least one visible line.
    pub(crate) fn visible_range(&self) -> Option<(usize, usize)> {
        let _ = self;
        todo!()
    }
}

/// Apply one input to the viewer without touching IO.
pub(crate) fn update(viewer: &mut Viewer, input: Input, renderer: &mut dyn CellRenderer) -> Effect {
    let _ = (viewer, input, renderer);
    todo!()
}

/// Renders cells of a notebook, handing pixels to the image renderer.
pub(crate) struct NotebookRenderer<'a> {
    /// Notebook the cells are read from.
    pub(crate) notebook: &'a Notebook,
    /// How code source is highlighted.
    pub(crate) language: SourceLanguage,
    /// Renderer that keeps the decoded pixels.
    pub(crate) images: &'a mut ImageRenderer,
}

impl CellRenderer for NotebookRenderer<'_> {
    fn render(&mut self, index: usize, options: &RenderOptions) -> RenderedCell {
        let _ = (index, options);
        todo!()
    }
}
