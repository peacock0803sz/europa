//! Helpers to draw images on a test screen.

use std::num::{NonZeroU16, NonZeroUsize};

use europa_core::{
    Cell, CellPixels, ImageSlot, Notebook, Output, RenderOptions, RenderedCell, SourceLanguage,
};
use image::DynamicImage;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::layout::Size;
use ratatui::Terminal;
use ratatui_image::errors::Errors;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::sliced::SlicedProtocol;

use crate::image_protocol::ColorDepth;
use crate::image_view::{Encoder, ImageRenderer, RatatuiEncoder};

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

/// What the cells of a frame hold, as one character each, for comparing and
/// for snapshots of the protocols whose data is long and not the same twice.
///
/// `I` is a cell with the data of iTerm2, `K` one with a Kitty placeholder,
/// `S` one with Sixel data, `-` a cell that is skipped, `x` the mark and `.`
/// anything else, tested in that order, because the wrapper of tmux can
/// surround the data of iTerm2 with an escape that Sixel data starts with.
/// The count of Sixel bands, and the row of the first column of each line of
/// Kitty placeholders, follow the lines, because the ids and the data are
/// different every time.
pub(crate) fn describe_protocol_frame(buffer: &Buffer) -> String {
    use ratatui::buffer::CellDiffOption;

    const PLACEHOLDER: char = '\u{10EEEE}';
    let area = buffer.area;
    let mut text = String::new();
    let mut bands = Vec::new();
    let mut rows = Vec::new();
    for y in area.top()..area.bottom() {
        let mut first_placeholder = None;
        for x in area.left()..area.right() {
            let cell = &buffer[(x, y)];
            let symbol = cell.symbol();
            text.push(if symbol.contains("]1337;File") {
                'I'
            } else if symbol.contains(PLACEHOLDER) {
                if first_placeholder.is_none() {
                    // The first cell of an image also carries the sequence that sends it,
                    // so the row is the character after the placeholder, not the second.
                    let mut chars = symbol.chars().skip_while(|c| *c != PLACEHOLDER);
                    first_placeholder = chars.nth(1).map(|row| row as u32);
                }
                'K'
            } else if symbol.contains("\u{1b}P") {
                bands.push(symbol.matches('-').count());
                'S'
            } else if cell.diff_option == CellDiffOption::Skip {
                '-'
            } else if symbol == "x" {
                'x'
            } else {
                '.'
            });
        }
        if let Some(row) = first_placeholder {
            rows.push(format!("{row:X}"));
        }
        text.push('\n');
    }
    if !bands.is_empty() {
        text.push_str(&format!("sixel bands {bands:?}\n"));
    }
    if !rows.is_empty() {
        text.push_str(&format!("kitty rows {}\n", rows.join(" ")));
    }
    text
}

/// An encoder that never succeeds.
pub(crate) struct FailingEncoder;

impl Encoder for FailingEncoder {
    fn encode(&self, _: &Picker, _: DynamicImage, _: Size) -> Result<SlicedProtocol, Errors> {
        Err(Errors::NoFontSize)
    }
}

/// An encoder that only succeeds for half-blocks, like a protocol that
/// cannot be encoded on this machine.
pub(crate) struct HalfblockOnly;

impl Encoder for HalfblockOnly {
    fn encode(
        &self,
        picker: &Picker,
        image: DynamicImage,
        size: Size,
    ) -> Result<SlicedProtocol, Errors> {
        if picker.protocol_type() == ProtocolType::Halfblocks {
            RatatuiEncoder.encode(picker, image, size)
        } else {
            Err(Errors::NoFontSize)
        }
    }
}

/// An encoder that fails for slots of one width, and encodes the others.
pub(crate) struct FailsForColumns(pub(crate) u16);

impl Encoder for FailsForColumns {
    fn encode(
        &self,
        picker: &Picker,
        image: DynamicImage,
        size: Size,
    ) -> Result<SlicedProtocol, Errors> {
        if size.width == self.0 {
            Err(Errors::NoFontSize)
        } else {
            RatatuiEncoder.encode(picker, image, size)
        }
    }
}

/// A renderer for `protocol` with an encoder of the test.
pub(crate) fn renderer_with(protocol: ProtocolType, encoder: Box<dyn Encoder>) -> ImageRenderer {
    ImageRenderer::with_parts(
        protocol,
        CellPixels::DEFAULT,
        ColorDepth::TrueColor,
        crate::image_cache::DEFAULT_LIMIT,
        encoder,
    )
}

/// A code cell as JSON with `source`, `execution_count` and `outputs`.
pub(crate) fn code_cell(source: &str, execution_count: Option<i64>, outputs: &[String]) -> String {
    let source_json = json_string(source);
    let count = match execution_count {
        Some(n) => n.to_string(),
        None => "null".to_owned(),
    };
    format!(
        r#"{{"cell_type":"code","metadata":{{}},"source":{source_json},"outputs":[{}],"execution_count":{count}}}"#,
        outputs.join(",")
    )
}

/// A Markdown cell as JSON with `source`.
pub(crate) fn markdown_cell(source: &str) -> String {
    let source_json = json_string(source);
    format!(r#"{{"cell_type":"markdown","metadata":{{}},"source":{source_json}}}"#)
}

/// A raw cell as JSON with `source`.
pub(crate) fn raw_cell(source: &str) -> String {
    let source_json = json_string(source);
    format!(r#"{{"cell_type":"raw","metadata":{{}},"source":{source_json}}}"#)
}

/// A cell of an unknown kind as JSON.
pub(crate) fn unknown_cell(cell_type: &str) -> String {
    let kind_json = json_string(cell_type);
    format!(r#"{{"cell_type":{kind_json},"metadata":{{}},"source":""}}"#)
}

/// A stream output as JSON.
pub(crate) fn stream(name: &str, text: &str) -> String {
    let name_json = json_string(name);
    let text_json = json_string(text);
    format!(r#"{{"output_type":"stream","name":{name_json},"text":{text_json}}}"#)
}

/// An error output as JSON.
pub(crate) fn error_output(ename: &str, evalue: &str) -> String {
    let ename_json = json_string(ename);
    let evalue_json = json_string(evalue);
    format!(
        r#"{{"output_type":"error","ename":{ename_json},"evalue":{evalue_json},"traceback":[]}}"#
    )
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A notebook made of the cell JSON fragments in `cells`.
pub(crate) fn notebook(cells: &[String]) -> Notebook {
    let text = format!(
        r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{}},"cells":[{}]}}"#,
        cells.join(",")
    );
    europa_core::parse(text.as_bytes()).expect("notebook loads")
}

/// A notebook where every cell holds 10 source lines and 20 stdout lines.
pub(crate) fn large_notebook(cells: usize) -> Notebook {
    let made: Vec<String> = (0..cells)
        .map(|_| {
            let source: String = (0..10).map(|n| format!("x = {n}\n")).collect();
            let out: String = (0..20).map(|n| format!("line {n}\n")).collect();
            code_cell(&source, Some(0), &[stream("stdout", &out)])
        })
        .collect();
    notebook(&made)
}

/// Records which cells were rendered, keeping pixels when asked.
pub(crate) struct RecordingRenderer<'a> {
    /// Notebook the cells are read from.
    pub(crate) notebook: &'a Notebook,
    /// How code source is highlighted.
    pub(crate) language: SourceLanguage,
    /// Indices rendered so far.
    pub(crate) calls: Vec<usize>,
    /// Keeps pixels when set.
    pub(crate) images: Option<ImageRenderer>,
}

impl crate::viewer::CellRenderer for RecordingRenderer<'_> {
    fn render(&mut self, index: usize, options: &RenderOptions) -> RenderedCell {
        self.calls.push(index);
        let language = self.language;
        let cell = &self.notebook.cells[index];
        let number = index + 1;
        let images = &mut self.images;
        crate::cell_display::cell_display(cell, number, language, options, &mut |source, image| {
            if let Some(kept) = images.as_mut() {
                kept.insert(source, image);
            }
        })
    }
}

impl RecordingRenderer<'_> {
    /// Recorded indices, emptied afterwards.
    pub(crate) fn take_calls(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.calls)
    }
}

/// Open `notebook` with `options` and a recorder without images.
pub(crate) fn open_with(
    notebook: &Notebook,
    columns: u16,
    rows: u16,
    options: RenderOptions,
) -> (crate::viewer::Viewer, RecordingRenderer<'_>) {
    open_with_images(notebook, columns, rows, options, None)
}

/// Open `notebook` with `options`, giving the recorder `images`.
fn open_with_images(
    notebook: &Notebook,
    columns: u16,
    rows: u16,
    options: RenderOptions,
    images: Option<ImageRenderer>,
) -> (crate::viewer::Viewer, RecordingRenderer<'_>) {
    let mut recorder = RecordingRenderer {
        notebook,
        language: notebook.source_language(),
        calls: Vec::new(),
        images,
    };
    let viewer = crate::viewer::Viewer::open(
        "test.ipynb".to_owned(),
        notebook.cells.len(),
        columns,
        rows,
        options,
        &mut recorder,
    );
    (viewer, recorder)
}

/// Open `notebook` for a dark background with default options.
pub(crate) fn open(
    notebook: &Notebook,
    columns: u16,
    rows: u16,
) -> (crate::viewer::Viewer, RecordingRenderer<'_>) {
    let width = NonZeroU16::new(columns.max(1)).expect("non-zero");
    open_with(notebook, columns, rows, RenderOptions::new(width))
}

/// Draw `viewer` and describe the frame after its position line.
pub(crate) fn frame_in(
    viewer: &crate::viewer::Viewer,
    notebook: &Notebook,
    renderer: &mut ImageRenderer,
) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(viewer.columns, viewer.rows)).expect("terminal");
    terminal
        .draw(|frame| {
            crate::viewer_view::view(viewer, notebook, frame.buffer_mut(), renderer);
        })
        .expect("draws");
    format!(
        "{:?}\n{}",
        viewer.position,
        describe(terminal.backend().buffer())
    )
}

/// Draw `viewer` with a half-block renderer.
pub(crate) fn frame(viewer: &crate::viewer::Viewer, notebook: &Notebook) -> String {
    let mut renderer = ImageRenderer::with_parts(
        ProtocolType::Halfblocks,
        CellPixels::DEFAULT,
        ColorDepth::TrueColor,
        crate::image_cache::DEFAULT_LIMIT,
        Box::new(RatatuiEncoder),
    );
    frame_in(viewer, notebook, &mut renderer)
}
