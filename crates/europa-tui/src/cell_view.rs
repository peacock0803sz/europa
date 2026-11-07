//! Drawing a rendered cell: its lines, and its images in their slots.

use europa_core::{Color, Output, RenderedCell, Style, StyledLine};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

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
    let shown = cell
        .lines
        .iter()
        .skip(scroll)
        .take(usize::from(area.height));
    for (row, line) in (0..).zip(shown) {
        let row_area = Rect::new(area.x, area.y + row, area.width, 1);
        // A line with no text paints nothing, not even its own style, so the
        // fill goes on the buffer before the text.
        if let Some(fill) = line.fill {
            buf.set_style(
                row_area,
                ratatui::style::Style::new().bg(ratatui_color(fill)),
            );
        }
        ratatui_line(line).render(row_area, buf);
    }
    for slot in &cell.images {
        let top = i32::try_from(slot.line).unwrap_or(i32::MAX)
            - i32::try_from(scroll).unwrap_or(i32::MAX);
        renderer.draw(buf, area, outputs, slot, top);
    }
}

/// Where the images of `cell` are on the screen, for the slots that are
/// visible in `area` when the cell is drawn from the line `scroll`.
pub(crate) fn visible_placements(area: Rect, cell: &RenderedCell, scroll: usize) -> Vec<Placement> {
    let height = i64::from(area.height);
    cell.images
        .iter()
        .filter_map(|slot| {
            let top = i64::try_from(slot.line).ok()? - i64::try_from(scroll).ok()?;
            let first = top.max(0);
            let last = (top + i64::from(slot.height)).min(height);
            if first >= last {
                return None;
            }
            Some(Placement {
                source: slot.source,
                x: area.x,
                y: area.y + u16::try_from(first).ok()?,
                width: slot.width.min(area.width),
                height: u16::try_from(last - first).ok()?,
            })
        })
        .collect()
}

fn ratatui_line(line: &StyledLine) -> Line<'static> {
    let spans: Vec<_> = line
        .spans
        .iter()
        .map(|span| Span::styled(span.text.clone(), ratatui_style(span.style)))
        .collect();
    Line::from(spans)
}

fn ratatui_style(style: Style) -> ratatui::style::Style {
    let mut drawn = ratatui::style::Style::new();
    if let Some(fg) = style.fg {
        drawn = drawn.fg(ratatui_color(fg));
    }
    if let Some(bg) = style.bg {
        drawn = drawn.bg(ratatui_color(bg));
    }
    let modifiers = [
        (style.bold, Modifier::BOLD),
        (style.dim, Modifier::DIM),
        (style.italic, Modifier::ITALIC),
        (style.underline, Modifier::UNDERLINED),
        (style.reverse, Modifier::REVERSED),
        (style.strikethrough, Modifier::CROSSED_OUT),
    ];
    for (on, modifier) in modifiers {
        if on {
            drawn = drawn.add_modifier(modifier);
        }
    }
    drawn
}

fn ratatui_color(color: Color) -> ratatui::style::Color {
    match color {
        Color::Indexed(index) => ratatui::style::Color::Indexed(index),
        Color::Rgb(red, green, blue) => ratatui::style::Color::Rgb(red, green, blue),
    }
}
