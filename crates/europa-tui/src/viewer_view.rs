//! Frame drawing of the viewer.

use std::num::NonZeroU16;
use std::path::Path;

use europa_core::{Notebook, SourceLanguage};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::image_view::{ImageRenderer, Placement};
use crate::viewer::Viewer;

/// File name for the status line, without control characters.
pub(crate) fn status_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|part| part.to_string_lossy().into_owned())
        .unwrap_or_default();
    let width = NonZeroU16::new(u16::MAX).expect("non-zero");
    let options = europa_core::RenderOptions::new(width);
    let lines = europa_render::render_source(&name, SourceLanguage::Plain, &options);
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push('\u{240a}');
        }
        for span in &line.spans {
            out.push_str(&span.text);
        }
    }
    if name.is_empty() {
        return out;
    }
    if lines.is_empty() {
        return String::new();
    }
    out
}

/// Draw the visible cells and the status line.
pub(crate) fn view(
    viewer: &Viewer,
    notebook: &Notebook,
    buf: &mut Buffer,
    renderer: &mut ImageRenderer,
) -> Vec<Placement> {
    if viewer.rows == 0 || viewer.columns == 0 {
        return Vec::new();
    }
    let height = viewer.rows.saturating_sub(1);
    let mut placements = Vec::new();
    let mut used: u16 = 0;
    let mut ordered: Vec<usize> = viewer.visible.keys().copied().collect();
    ordered.sort_unstable();
    for (order, index) in ordered.iter().enumerate() {
        let Some(cell) = viewer.visible.get(index) else {
            continue;
        };
        let scroll = if order == 0 { viewer.position.row } else { 0 };
        let remaining = height.saturating_sub(used);
        if remaining == 0 {
            break;
        }
        let cell_lines = cell.lines.len();
        let shown = (cell_lines.saturating_sub(scroll) as u16).min(remaining);
        if shown == 0 {
            continue;
        }
        let area = Rect::new(0, used, viewer.columns, shown);
        let outputs: &[europa_core::Output] = match notebook.cells.get(*index) {
            Some(europa_core::Cell::Code(code)) => &code.outputs,
            _ => &[],
        };
        placements.extend(crate::cell_view::visible_placements(area, cell, scroll));
        crate::cell_view::draw_cell(buf, area, cell, outputs, scroll, renderer);
        used += shown;
    }
    if viewer.cell_count == 0 && height >= 1 {
        let style = Style::new().add_modifier(Modifier::DIM);
        buf.set_stringn(0, 0, "[no cells]", usize::from(viewer.columns), style);
    }
    draw_status(viewer, buf);
    placements
}

fn draw_status(viewer: &Viewer, buf: &mut Buffer) {
    if viewer.rows == 0 || viewer.columns == 0 {
        return;
    }
    let count = viewer.cell_count;
    let range = match viewer.visible_range() {
        None => format!("/{count}"),
        Some((first, last)) if first == last => format!("{}/{count}", first + 1),
        Some((first, last)) => format!("{}-{}/{}", first + 1, last + 1, count),
    };
    let width = usize::from(viewer.columns);
    let range_shown: String = if range.chars().count() > width {
        range.chars().take(width).collect()
    } else {
        range
    };
    let remaining = width.saturating_sub(range_shown.chars().count());
    let file_shown: String = if viewer.file_name.chars().count() > remaining {
        viewer.file_name.chars().take(remaining).collect()
    } else {
        viewer.file_name.clone()
    };
    let gaps = width
        .saturating_sub(file_shown.chars().count())
        .saturating_sub(range_shown.chars().count());
    let mut line = file_shown;
    line.extend(std::iter::repeat_n(' ', gaps));
    line.push_str(&range_shown);
    let style = Style::new().add_modifier(Modifier::REVERSED);
    let y = viewer.rows - 1;
    buf.set_stringn(0, y, &line, width, style);
}
