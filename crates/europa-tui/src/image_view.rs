//! Drawing the slots that `europa-render` keeps for images.

use europa_core::{CellPixels, DecodedImage, ImageSlot, Output};
use image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
use ratatui_image::errors::Errors;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::sliced::SlicedProtocol;

use crate::image_cache::ImageCache;
use crate::image_protocol::ColorDepth;

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
        todo!()
    }
}

/// A picker for a protocol and a cell size that were decided elsewhere.
fn build_picker(cell_pixels: CellPixels, protocol: ProtocolType) -> Picker {
    todo!()
}

/// Puts the image at the top left of a white image the size of the slot,
/// shrinking or enlarging it only when its size is not that of the slot.
fn fit_to_slot(
    image: &DecodedImage,
    columns: u16,
    rows: u16,
    cell_pixels: CellPixels,
) -> DynamicImage {
    todo!()
}

/// The color of the 256-color palette nearest to the given one.
fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    todo!()
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
    /// A renderer that does not touch the terminal.
    pub(crate) fn with_parts(
        protocol: ProtocolType,
        cell_pixels: CellPixels,
        color_depth: ColorDepth,
        cache_limit: usize,
        encoder: Box<dyn Encoder>,
    ) -> Self {
        todo!()
    }

    /// The protocol in use.
    pub(crate) fn protocol(&self) -> ProtocolType {
        todo!()
    }

    /// The size of a cell, for the options of the renderer.
    pub(crate) fn cell_pixels(&self) -> CellPixels {
        todo!()
    }

    /// Keeps the pixels of an image for drawing.
    pub(crate) fn insert(&mut self, source: u64, image: DecodedImage) {
        todo!()
    }

    /// Takes a new size of a cell from the window. Returns whether it
    /// changed, in which case the cells have to be rendered again.
    pub(crate) fn apply_window(&mut self, window: Option<CellPixels>) -> bool {
        todo!()
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
        todo!()
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
