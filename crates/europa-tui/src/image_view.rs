//! Drawing the slots that `europa-render` keeps for images.

use std::io::IsTerminal;

use europa_core::{CellPixels, DecodedImage, ImageProtocolChoice, ImageSlot, Output};
use image::imageops::FilterType;
use image::{DynamicImage, RgbImage};
use ratatui::backend::{Backend, ClearType};
use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::Terminal;
use ratatui_image::errors::Errors;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::sliced::{SignedPosition, SlicedImage, SlicedProtocol};
use ratatui_image::{FontSize, Resize};

use crate::image_cache::{EncodeKey, Encoded, ImageCache, DEFAULT_LIMIT};
use crate::image_protocol::{
    cell_pixels_from, color_depth, plan_selection, probe_from, ColorDepth, Selection,
};

/// Where an image is on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placement {
    /// The image, as the key of the cache.
    pub(crate) source: u64,
    /// Column of the visible part.
    pub(crate) x: u16,
    /// Line of the visible part.
    pub(crate) y: u16,
    /// Columns of the visible part.
    pub(crate) width: u16,
    /// Lines of the visible part.
    pub(crate) height: u16,
}

/// How often images were decoded and encoded while drawing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ImageStats {
    /// Decodes done by `draw`. The decoding of `render_outputs_with_images`
    /// is not counted.
    pub(crate) decodes: u64,
    /// Encodings for the terminal.
    pub(crate) encodes: u64,
}

/// Turns an image into data for a protocol. Tests replace it with one that
/// fails.
pub(crate) trait Encoder {
    /// Encodes `image` for a slot of `size` cells.
    fn encode(
        &self,
        picker: &Picker,
        image: DynamicImage,
        size: Size,
    ) -> Result<SlicedProtocol, Errors>;
}

/// The encoder of `ratatui-image`.
pub(crate) struct RatatuiEncoder;

impl Encoder for RatatuiEncoder {
    fn encode(
        &self,
        picker: &Picker,
        image: DynamicImage,
        size: Size,
    ) -> Result<SlicedProtocol, Errors> {
        SlicedProtocol::new_with_resize(picker, image, size, Resize::Fit(None))
    }
}

/// A picker for a protocol and a cell size that were decided elsewhere.
pub(crate) fn build_picker(cell_pixels: CellPixels, protocol: ProtocolType) -> Picker {
    // `from_fontsize` is the only way to give a picker a cell size that was
    // not found by querying the terminal, which is not done again after the
    // window is resized.
    #[allow(deprecated)]
    let mut picker = Picker::from_fontsize(FontSize::new(
        cell_pixels.width.get(),
        cell_pixels.height.get(),
    ));
    picker.set_protocol_type(protocol);
    picker
}

/// Puts the image at the top left of a white image the size of the slot,
/// shrinking or enlarging it only when its size is not that of the slot.
///
/// The white margin keeps the data of every protocol on the grid of cells,
/// and makes the background the same as for a transparent image.
pub(crate) fn fit_to_slot(
    image: &DecodedImage,
    columns: u16,
    rows: u16,
    cell_pixels: CellPixels,
) -> DynamicImage {
    let cell_width = u32::from(cell_pixels.width.get());
    let cell_height = u32::from(cell_pixels.height.get());
    let slot_width = u32::from(columns.max(1)) * cell_width;
    let slot_height = u32::from(rows.max(1)) * cell_height;
    let mut slot = RgbImage::from_pixel(slot_width, slot_height, image::Rgb([255, 255, 255]));

    let Some(source) = RgbImage::from_raw(image.width, image.height, image.rgb.clone()) else {
        return slot.into();
    };
    let natural = (
        image.width.div_ceil(cell_width),
        image.height.div_ceil(cell_height),
    );
    // A slot is the size of the image rounded up to whole cells, and scaling
    // to fill that remainder would blur the image and enlarge a small one.
    let placed = if natural == (u32::from(columns), u32::from(rows)) {
        source
    } else {
        let scale = (f64::from(slot_width) / f64::from(image.width))
            .min(f64::from(slot_height) / f64::from(image.height));
        let scaled = |side: u32| ((f64::from(side) * scale).round() as u32).max(1);
        image::imageops::resize(
            &source,
            scaled(image.width).min(slot_width),
            scaled(image.height).min(slot_height),
            FilterType::Triangle,
        )
    };
    image::imageops::replace(&mut slot, &placed, 0, 0);
    slot.into()
}

/// The color of the 256-color palette nearest to the given one.
///
/// The first 16 colors are not used, because terminals let their users
/// change them.
pub(crate) fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let distance = |candidate: (u8, u8, u8)| {
        let square = |from: u8, to: u8| u32::from(from.abs_diff(to)).pow(2);
        square(r, candidate.0) + square(g, candidate.1) + square(b, candidate.2)
    };
    let mut best = (u32::MAX, 16u8);
    for (red, &r_level) in LEVELS.iter().enumerate() {
        for (green, &g_level) in LEVELS.iter().enumerate() {
            for (blue, &b_level) in LEVELS.iter().enumerate() {
                let index = 16 + 36 * red + 6 * green + blue;
                let found = distance((r_level, g_level, b_level));
                // Ties keep the smaller index, which comes first.
                if found < best.0 {
                    best = (found, u8::try_from(index).expect("below 232"));
                }
            }
        }
    }
    for step in 0..24u8 {
        let level = 8 + 10 * step;
        let found = distance((level, level, level));
        if found < best.0 {
            best = (found, 232 + step);
        }
    }
    best.1
}

/// The size of a cell from the window, when the terminal says it.
fn window_cell_pixels() -> Option<CellPixels> {
    let window = ratatui::crossterm::terminal::window_size().ok()?;
    cell_pixels_from(window.columns, window.rows, window.width, window.height)
}

/// Draws images in the slots of rendered cells.
pub(crate) struct ImageRenderer {
    picker: Picker,
    color_depth: ColorDepth,
    cell_pixels: CellPixels,
    cache: ImageCache,
    encoder: Box<dyn Encoder>,
    stats: ImageStats,
    last_placements: Vec<Placement>,
}

impl ImageRenderer {
    /// Chooses a protocol and finds the size of a cell.
    ///
    /// The terminal is queried only when `choice` leaves it open and both the
    /// input and the output are a terminal. Call this after `ratatui::init`
    /// and before the first event is read, because the answer of the terminal
    /// comes through the input.
    pub(crate) fn new(choice: ImageProtocolChoice) -> Self {
        let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
        let selection = plan_selection(choice, interactive);
        let probe = match selection {
            Selection::Halfblock => None,
            // A terminal that does not answer still gives `Ok`, with what the
            // environment suggests, so `None` is only a failure to read or
            // write.
            Selection::Query { .. } => Picker::from_query_stdio()
                .ok()
                .map(|picker| probe_from(picker.protocol_type(), picker.capabilities())),
        };
        let (protocol, cell_pixels) = selection.resolve(probe, window_cell_pixels());
        Self::with_parts(
            protocol,
            cell_pixels,
            color_depth(|name| std::env::var(name).ok()),
            DEFAULT_LIMIT,
            Box::new(RatatuiEncoder),
        )
    }

    /// A renderer that does not touch the terminal.
    pub(crate) fn with_parts(
        protocol: ProtocolType,
        cell_pixels: CellPixels,
        color_depth: ColorDepth,
        cache_limit: usize,
        encoder: Box<dyn Encoder>,
    ) -> Self {
        Self {
            picker: build_picker(cell_pixels, protocol),
            color_depth,
            cell_pixels,
            cache: ImageCache::new(cache_limit),
            encoder,
            stats: ImageStats::default(),
            last_placements: Vec::new(),
        }
    }

    /// The protocol in use.
    pub(crate) fn protocol(&self) -> ProtocolType {
        self.picker.protocol_type()
    }

    /// The size of a cell, for the options of the renderer.
    pub(crate) fn cell_pixels(&self) -> CellPixels {
        self.cell_pixels
    }

    /// Keeps the pixels of an image for drawing.
    pub(crate) fn insert(&mut self, source: u64, image: DecodedImage) {
        self.cache.insert(source, image);
    }

    /// Takes a new size of a cell from the window. Returns whether it
    /// changed, in which case the cells have to be rendered again.
    pub(crate) fn apply_window(&mut self, window: Option<CellPixels>) -> bool {
        match window {
            Some(window) if window != self.cell_pixels => {
                self.cell_pixels = window;
                self.picker = build_picker(window, self.picker.protocol_type());
                true
            }
            _ => false,
        }
    }

    /// Reads the size of the window again, without a query to the terminal.
    pub(crate) fn on_resize(&mut self) -> bool {
        self.apply_window(window_cell_pixels())
    }

    /// Draws the image of `slot`. `top` is the line of the slot counted from
    /// the top of `area`, and is negative when the slot starts above it.
    pub(crate) fn draw(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        outputs: &[Output],
        slot: &ImageSlot,
        top: i32,
    ) {
        let height = i32::from(slot.height);
        if area.width == 0 || area.height == 0 || top >= i32::from(area.height) || top + height <= 0
        {
            return;
        }
        // Positions are `i16` in the widget, and a slot that tall cannot be
        // given one.
        let (Ok(position), true) = (i16::try_from(top), slot.height <= i16::MAX as u16) else {
            draw_notice(buf, area, slot, top);
            return;
        };
        if self.cache.pixels(slot.source).is_none() {
            let Some(image) = europa_render::decode_image(outputs, slot) else {
                draw_notice(buf, area, slot, top);
                return;
            };
            self.stats.decodes += 1;
            self.cache.insert(slot.source, image);
        }
        // Kitty numbers the lines and the columns of a slot with a table that
        // has 297 entries, and what is beyond it is drawn at the wrong place
        // or not at all.
        let protocol_type = match self.picker.protocol_type() {
            ProtocolType::Kitty if slot.width > KITTY_LIMIT || slot.height > KITTY_LIMIT => {
                ProtocolType::Halfblocks
            }
            other => other,
        };
        let key = EncodeKey {
            columns: slot.width,
            rows: slot.height,
            cell_pixels: self.cell_pixels,
            protocol: protocol_type,
        };
        if self.cache.encoded(slot.source, &key).is_none() {
            let Some(pixels) = self.cache.pixels(slot.source) else {
                draw_notice(buf, area, slot, top);
                return;
            };
            let image = fit_to_slot(&pixels, slot.width, slot.height, self.cell_pixels);
            self.stats.encodes += 1;
            let picker = if protocol_type == self.picker.protocol_type() {
                None
            } else {
                Some(build_picker(self.cell_pixels, protocol_type))
            };
            let picker = picker.as_ref().unwrap_or(&self.picker);
            match self
                .encoder
                .encode(picker, image, Size::new(slot.width, slot.height))
            {
                Ok(protocol) => {
                    self.cache
                        .store_encoded(slot.source, key.clone(), Encoded::Ready(protocol));
                }
                // The fall back for a failure is added with the handling of
                // failures; here it is a notice.
                Err(_) => {
                    draw_notice(buf, area, slot, top);
                    return;
                }
            }
        }
        let Some(Encoded::Ready(protocol)) = self.cache.encoded(slot.source, &key) else {
            draw_notice(buf, area, slot, top);
            return;
        };
        draw_sliced(buf, area, slot, top, position, protocol);
        if self.color_depth == ColorDepth::Indexed256
            && matches!(protocol, SlicedProtocol::Halfblocks(_))
        {
            reduce_to_256(buf, area, slot, top);
        }
    }

    /// How often images were decoded and encoded.
    #[cfg(test)]
    pub(crate) fn stats(&self) -> ImageStats {
        self.stats
    }

    /// The size of the cache, in bytes.
    #[cfg(test)]
    pub(crate) fn cached_bytes(&self) -> usize {
        self.cache.total_bytes()
    }
}

/// The most lines or columns of a slot that Kitty can number.
const KITTY_LIMIT: u16 = 297;

/// Draws what is visible of an encoded slot, the way its protocol needs.
///
/// A slot can be wider than the area for a moment after the terminal
/// shrank, so the width is cut to the area.
fn draw_sliced(
    buf: &mut Buffer,
    area: Rect,
    slot: &ImageSlot,
    top: i32,
    position: i16,
    protocol: &SlicedProtocol,
) {
    let (columns, lines) = visible(area, slot, top);
    let skip = usize::try_from(-top).unwrap_or(0);
    let shown = usize::from(lines.end.saturating_sub(lines.start));
    let drop = usize::from(slot.height).saturating_sub(skip + shown);
    let rect = Rect::new(
        columns.start,
        lines.start,
        columns.end.saturating_sub(columns.start),
        lines.end.saturating_sub(lines.start),
    );
    match protocol {
        // The widget of the crate draws min(skip, drop) lines of iTerm2 and
        // Sixel outside of the area when a slot sticks out on both sides,
        // which is what scrolling through a tall image does, so those two are
        // drawn here from the parts that the crate keeps.
        SlicedProtocol::Sliced(rows) => {
            debug_assert_eq!(rows.len(), usize::from(slot.height));
            let visible_rows = rows.iter().skip(skip).take(shown);
            for (line, row) in (rect.y..).zip(visible_rows) {
                let line_area = Rect::new(rect.x, line, rect.width, 1);
                ratatui::widgets::Widget::render(ratatui_image::Image::new(row), line_area, buf);
            }
        }
        SlicedProtocol::Sixel(sixel) => {
            sixel
                .borrow_dependent()
                .render(rect, buf, skip, skip + drop);
        }
        SlicedProtocol::Kitty(_) | SlicedProtocol::Halfblocks(_) => {
            ratatui::widgets::Widget::render(
                SlicedImage::new(protocol, SignedPosition::from((0, position))),
                area,
                buf,
            );
        }
    }
}

/// The part of the slot that is on the screen, as columns and lines of the
/// buffer.
fn visible(area: Rect, slot: &ImageSlot, top: i32) -> (std::ops::Range<u16>, std::ops::Range<u16>) {
    let first = top.max(0);
    let last = (top + i32::from(slot.height)).min(i32::from(area.height));
    let to_u16 = |value: i32| u16::try_from(value).unwrap_or(u16::MAX);
    let columns = area.x..area.x.saturating_add(slot.width.min(area.width));
    let lines = area.y.saturating_add(to_u16(first))..area.y.saturating_add(to_u16(last));
    (columns, lines)
}

/// Replaces the 24-bit colors of the slot with the nearest of the 256.
///
/// Cells are reached by `cell_mut` because the terminal can have shrunk, and
/// indexing outside of the buffer panics.
fn reduce_to_256(buf: &mut Buffer, area: Rect, slot: &ImageSlot, top: i32) {
    let (columns, lines) = visible(area, slot, top);
    for y in lines {
        for x in columns.clone() {
            let Some(cell) = buf.cell_mut((x, y)) else {
                continue;
            };
            for color in [&mut cell.fg, &mut cell.bg] {
                if let Color::Rgb(r, g, b) = *color {
                    *color = Color::Indexed(nearest_256(r, g, b));
                }
            }
        }
    }
}

/// Names the image in the first visible line of its slot, for when it cannot
/// be drawn.
fn draw_notice(buf: &mut Buffer, area: Rect, slot: &ImageSlot, top: i32) {
    let (columns, lines) = visible(area, slot, top);
    if lines.is_empty() {
        return;
    }
    let y = lines.start;
    let text = format!("[{}: cannot draw]", slot.mime.as_str());
    let width = usize::from(columns.end.saturating_sub(columns.start));
    buf.set_stringn(
        columns.start,
        y,
        text,
        width,
        Style::new().add_modifier(Modifier::DIM),
    );
}

impl ImageRenderer {
    /// Saves where the images are on this frame, and says whether the screen
    /// has to be cleared before they are drawn.
    ///
    /// Call this once for every frame, before drawing, with the placements of
    /// every visible image. An image of Sixel or iTerm2 is not part of the
    /// cells, so the terminal keeps it after the cells under it are drawn
    /// over, and it is only gone when the screen is cleared. Kitty and
    /// half-blocks are made of cells and need no clearing. When this returns
    /// `true`, call [`clear_screen`] and then draw.
    pub(crate) fn placements_changed(&mut self, placements: &[Placement]) -> bool {
        let changed = self.last_placements != placements;
        // Kept for every protocol, because a failed write is answered by
        // switching the images that were on screen, whatever drew them.
        self.last_placements = placements.to_vec();
        changed
            && matches!(
                self.picker.protocol_type(),
                ProtocolType::Sixel | ProtocolType::Iterm2
            )
    }
}

/// Empties the screen and both buffers, so that the next frame draws every
/// cell again.
///
/// `Terminal::clear` asks the terminal for the position of the cursor, and a
/// terminal that does not answer makes that wait for two seconds, which would
/// stop scrolling every time an image moves. The cursor is hidden at every
/// frame, so its position does not need to be kept.
pub(crate) fn clear_screen<B: Backend>(terminal: &mut Terminal<B>) -> Result<(), B::Error> {
    terminal.current_buffer_mut().reset();
    terminal.backend_mut().clear_region(ClearType::All)?;
    terminal.swap_buffers();
    Ok(())
}

impl ImageRenderer {
    /// Switches the images that were passed to the last call of
    /// [`ImageRenderer::placements_changed`] to half-blocks, for when writing
    /// a frame failed.
    ///
    /// The failure does not say which image caused it, so every image that
    /// was on screen is switched. Call [`clear_screen`] after this and draw
    /// the frame again. An image that was not on screen keeps its protocol.
    pub(crate) fn fall_back_visible(&mut self) {
        todo!()
    }
}
