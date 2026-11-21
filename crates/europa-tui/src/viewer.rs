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
    /// Move down one line.
    Down,
    /// Move up one line.
    Up,
    /// Move to the next cell.
    NextCell,
    /// Move to the previous cell.
    PrevCell,
    /// Wait for the next key to decide on going to the top.
    GPrefix,
    /// Move to the bottom.
    Bottom,
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
        let width = std::num::NonZeroU16::new(columns.max(1)).expect("non-zero");
        let mut options = options;
        options.width = width;
        let mut viewer = Viewer {
            file_name,
            cell_count,
            position: Position { cell: 0, row: 0 },
            columns,
            rows,
            options,
            visible: std::collections::BTreeMap::new(),
        };
        viewer.fill_visible(renderer);
        viewer
    }

    /// First and last cells with at least one visible line.
    pub(crate) fn visible_range(&self) -> Option<(usize, usize)> {
        let first = *self.visible.keys().next()?;
        let last = *self.visible.keys().next_back()?;
        Some((first, last))
    }

    fn viewport_lines(&self) -> usize {
        usize::from(self.rows.saturating_sub(1))
    }

    fn visible_sum(&self) -> usize {
        let mut sum = 0usize;
        let mut first = true;
        let mut ordered: Vec<usize> = self.visible.keys().copied().collect();
        ordered.sort_unstable();
        for index in ordered {
            if let Some(cell) = self.visible.get(&index) {
                if first {
                    sum += cell.lines.len().saturating_sub(self.position.row);
                    first = false;
                } else {
                    sum += cell.lines.len();
                }
            }
        }
        sum
    }

    fn last_visible(&self) -> Option<usize> {
        self.visible.keys().copied().max()
    }

    fn bottom(&mut self, renderer: &mut dyn CellRenderer) -> Position {
        let height = self.viewport_lines();
        if height == 0 || self.columns == 0 || self.cell_count == 0 {
            return Position { cell: 0, row: 0 };
        }
        let mut sum = 0usize;
        let mut index = self.cell_count;
        while index > 0 {
            index -= 1;
            if !self.visible.contains_key(&index) {
                let rendered = renderer.render(index, &self.options);
                self.visible.insert(index, rendered);
            }
            let cell_height = self
                .visible
                .get(&index)
                .map(|cell| cell.lines.len())
                .unwrap_or(0);
            sum += cell_height;
            if sum >= height {
                return Position {
                    cell: index,
                    row: sum - height,
                };
            }
        }
        Position { cell: 0, row: 0 }
    }

    fn fill_visible(&mut self, renderer: &mut dyn CellRenderer) {
        let height = self.viewport_lines();
        if height == 0 || self.columns == 0 || self.cell_count == 0 {
            self.visible.clear();
            return;
        }
        let mut used = 0usize;
        let mut index = self.position.cell;
        let mut first = true;
        while index < self.cell_count {
            if !self.visible.contains_key(&index) {
                let rendered = renderer.render(index, &self.options);
                self.visible.insert(index, rendered);
            }
            let cell_height = self
                .visible
                .get(&index)
                .map(|cell| cell.lines.len())
                .unwrap_or(0);
            let shown = if first {
                cell_height.saturating_sub(self.position.row)
            } else {
                cell_height
            };
            used += shown;
            first = false;
            if used >= height || index + 1 >= self.cell_count {
                index += 1;
                break;
            }
            index += 1;
        }
        let end = index;
        self.visible
            .retain(|key, _| *key >= self.position.cell && *key < end);
    }
}

/// Apply one input to the viewer without touching IO.
pub(crate) fn update(viewer: &mut Viewer, input: Input, renderer: &mut dyn CellRenderer) -> Effect {
    match input {
        Input::Key(Key::Quit) => Effect::Quit,
        Input::Key(Key::Down) => {
            step_down(viewer, renderer);
            Effect::None
        }
        Input::Key(Key::Up) => {
            step_up(viewer, renderer);
            Effect::None
        }
        Input::Key(Key::NextCell | Key::PrevCell | Key::GPrefix | Key::Bottom) => Effect::None,
        Input::Key(Key::Other) => Effect::None,
        Input::Resize {
            columns,
            rows,
            cell_pixels,
        } => {
            if columns != viewer.columns || cell_pixels != viewer.options.cell_pixels {
                let old_height = viewer
                    .visible
                    .get(&viewer.position.cell)
                    .map(|cell| cell.lines.len());
                viewer.visible.clear();
                viewer.columns = columns;
                viewer.rows = rows;
                viewer.options.width = std::num::NonZeroU16::new(columns.max(1)).expect("non-zero");
                viewer.options.cell_pixels = cell_pixels;
                let first = viewer.position.cell;
                let new_height = if viewer.cell_count == 0 {
                    0
                } else {
                    let index = first.min(viewer.cell_count - 1);
                    let rendered = renderer.render(index, &viewer.options);
                    viewer.visible.insert(index, rendered);
                    viewer
                        .visible
                        .get(&index)
                        .map(|cell| cell.lines.len())
                        .unwrap_or(0)
                };
                viewer.position = match (old_height, new_height) {
                    (Some(old), new) if old > 0 => Position {
                        cell: first,
                        row: viewer.position.row * new / old,
                    },
                    _ => Position {
                        cell: first,
                        row: 0,
                    },
                };
                if viewer.cell_count > 0 {
                    let index = viewer.position.cell.min(viewer.cell_count - 1);
                    let height = viewer
                        .visible
                        .get(&index)
                        .map(|cell| cell.lines.len())
                        .unwrap_or(0);
                    if viewer.position.row >= height.max(1) {
                        viewer.position.row = height.saturating_sub(1);
                    }
                }
                viewer.fill_visible(renderer);
                if needs_bottom(viewer) {
                    viewer.position = viewer.bottom(renderer);
                    viewer.fill_visible(renderer);
                }
            } else if rows != viewer.rows {
                viewer.rows = rows;
                viewer.fill_visible(renderer);
                if needs_bottom(viewer) {
                    viewer.position = viewer.bottom(renderer);
                    viewer.fill_visible(renderer);
                }
            }
            Effect::None
        }
    }
}

fn needs_bottom(viewer: &Viewer) -> bool {
    if viewer.cell_count == 0 || viewer.viewport_lines() == 0 {
        return false;
    }
    let Some(last) = viewer.last_visible() else {
        return false;
    };
    if last + 1 < viewer.cell_count {
        return false;
    }
    viewer.visible_sum() < viewer.viewport_lines()
}

fn step_down(viewer: &mut Viewer, renderer: &mut dyn CellRenderer) {
    let height = viewer.viewport_lines();
    if height == 0 || viewer.cell_count == 0 || viewer.columns == 0 {
        return;
    }
    let sum = viewer.visible_sum();
    let last = viewer.last_visible().unwrap_or(viewer.position.cell);
    if sum <= height && last + 1 >= viewer.cell_count {
        return;
    }
    let first_height = viewer
        .visible
        .get(&viewer.position.cell)
        .map(|cell| cell.lines.len())
        .unwrap_or(0);
    if viewer.position.row + 1 < first_height {
        viewer.position.row += 1;
    } else if viewer.position.cell + 1 < viewer.cell_count {
        viewer.position.cell += 1;
        viewer.position.row = 0;
    } else {
        return;
    }
    viewer.fill_visible(renderer);
}

fn step_up(viewer: &mut Viewer, renderer: &mut dyn CellRenderer) {
    let height = viewer.viewport_lines();
    if height == 0 || viewer.cell_count == 0 || viewer.columns == 0 {
        return;
    }
    if viewer.position.row > 0 {
        viewer.position.row -= 1;
    } else if viewer.position.cell > 0 {
        let prev = viewer.position.cell - 1;
        if !viewer.visible.contains_key(&prev) {
            let rendered = renderer.render(prev, &viewer.options);
            viewer.visible.insert(prev, rendered);
        }
        let prev_height = viewer
            .visible
            .get(&prev)
            .map(|cell| cell.lines.len())
            .unwrap_or(1);
        viewer.position.cell = prev;
        viewer.position.row = prev_height.saturating_sub(1);
    } else {
        return;
    }
    viewer.fill_visible(renderer);
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
        crate::cell_display::cell_display(
            &self.notebook.cells[index],
            index + 1,
            self.language,
            options,
            &mut |source, image| {
                self.images.insert(source, image);
            },
        )
    }
}
