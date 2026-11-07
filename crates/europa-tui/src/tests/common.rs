//! Helpers to draw images on a test screen.

use std::num::{NonZeroU16, NonZeroUsize};

use europa_core::{Cell, CellPixels, ImageSlot, Output, RenderOptions, RenderedCell};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::Terminal;
use ratatui_image::picker::ProtocolType;

use crate::image_protocol::ColorDepth;
use crate::image_view::{ImageRenderer, RatatuiEncoder};

/// A PNG whose color follows from the coordinates.
pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([
            (x * 3 % 256) as u8,
            (y * 5 % 256) as u8,
            ((x + y) % 256) as u8,
        ])
    });
    encode(image, image::ImageFormat::Png)
}

/// A PNG all of whose pixels are fully transparent.
pub(crate) fn png_transparent(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 0]));
    encode(image, image::ImageFormat::Png)
}

/// A JPEG of one color, which the encoder reproduces to within a few levels
/// whatever the size.
pub(crate) fn jpeg_solid(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(width, height, image::Rgb([216, 40, 40]));
    encode(image, image::ImageFormat::Jpeg)
}

fn encode(image: impl Into<image::DynamicImage>, format: image::ImageFormat) -> Vec<u8> {
    let mut bytes = Vec::new();
    image
        .into()
        .write_to(&mut std::io::Cursor::new(&mut bytes), format)
        .expect("encodes");
    bytes
}

/// A display data output holding the image `bytes` under `mime`, with the
/// metadata `metadata` as a JSON object for that type.
pub(crate) fn image_output_with(mime: &str, bytes: &[u8], metadata: &str) -> Output {
    let data = base64_simd::STANDARD.encode_to_string(bytes);
    let json = format!(
        r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{}},"cells":[{{"cell_type":"code","id":"c1","metadata":{{}},"source":"","outputs":[{{"output_type":"display_data","data":{{"{mime}":"{data}"}},"metadata":{{"{mime}":{metadata}}}}}],"execution_count":null}}]}}"#
    );
    let notebook = europa_core::parse(json.as_bytes()).expect("fixture notebook loads");
    let Some(Cell::Code(code)) = notebook.cells.first() else {
        panic!("no code cell");
    };
    code.outputs[0].clone()
}

/// A display data output holding the PNG `bytes`.
pub(crate) fn png_output(bytes: &[u8]) -> Output {
    image_output_with("image/png", bytes, "{}")
}

/// A display data output holding the JPEG `bytes`.
pub(crate) fn jpeg_output(bytes: &[u8]) -> Output {
    image_output_with("image/jpeg", bytes, "{}")
}

/// A renderer for `protocol` that works on cells of 10 by 20 pixels.
pub(crate) fn renderer(protocol: ProtocolType, depth: ColorDepth, limit: usize) -> ImageRenderer {
    ImageRenderer::with_parts(
        protocol,
        CellPixels::DEFAULT,
        depth,
        limit,
        Box::new(RatatuiEncoder),
    )
}

/// Renders the outputs the way the application does, handing the pixels to
/// the renderer.
pub(crate) fn render(
    renderer: &mut ImageRenderer,
    outputs: &[Output],
    width: u16,
    max_lines: usize,
) -> RenderedCell {
    let options = RenderOptions {
        max_output_lines: NonZeroUsize::new(max_lines).expect("non-zero"),
        cell_pixels: renderer.cell_pixels(),
        ..RenderOptions::new(NonZeroU16::new(width).expect("non-zero"))
    };
    europa_render::render_outputs_with_images(outputs, &options, &mut |source, image| {
        renderer.insert(source, image);
    })
}

/// The slot of the first image of the outputs, rendered at the width 80.
pub(crate) fn first_slot(renderer: &mut ImageRenderer, outputs: &[Output]) -> ImageSlot {
    let cell = render(renderer, outputs, 80, 200);
    cell.images[0]
}

/// Draws the slot on a screen of `screen` columns and lines that is marked
/// with `x` everywhere, and returns the frame that was completed.
///
/// The buffer of the completed frame is taken instead of the one of the
/// backend, because the latter has left out the cells that were skipped.
pub(crate) fn draw_frame(
    renderer: &mut ImageRenderer,
    outputs: &[Output],
    slot: &ImageSlot,
    area: Rect,
    top: i32,
    screen: (u16, u16),
) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(screen.0, screen.1)).expect("terminal");
    let frame = terminal
        .draw(|frame| {
            for cell in &mut frame.buffer_mut().content {
                cell.set_symbol("x");
            }
            renderer.draw(frame.buffer_mut(), area, outputs, slot, top);
        })
        .expect("draws");
    frame.buffer.clone()
}

/// The debug text of a buffer, without the underline color that building the
/// whole workspace adds to every cell.
pub(crate) fn describe(buffer: &Buffer) -> String {
    format!("{buffer:?}").replace(", underline: Reset", "")
}

/// A PNG of one color, so that images of one size can still differ.
pub(crate) fn png_solid(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(width, height, image::Rgb(color));
    encode(image, image::ImageFormat::Png)
}

/// A stdout output with `text`.
pub(crate) fn stdout(text: &str) -> Output {
    Output::new_stream("stdout", text)
}
