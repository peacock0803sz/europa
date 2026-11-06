#![allow(dead_code)]

use std::num::{NonZeroU16, NonZeroUsize};

use europa_core::{
    Background, Cell, Color, DecodedImage, JsonMap, Notebook, Output, RenderOptions, RenderedCell,
    SourceLanguage, Style, StyledLine,
};
use unicode_width::UnicodeWidthStr;

/// Options for an area `width` columns wide, with the default limit and a dark
/// background.
pub fn opts(width: u16) -> RenderOptions {
    RenderOptions::new(NonZeroU16::new(width).expect("width is not zero"))
}

/// Like [`opts`], but with the line limit `max`.
pub fn limited(width: u16, max: usize) -> RenderOptions {
    RenderOptions {
        max_output_lines: NonZeroUsize::new(max).expect("limit is not zero"),
        ..opts(width)
    }
}

/// Like [`opts`], but for a light background.
pub fn light(width: u16) -> RenderOptions {
    RenderOptions {
        background: Background::Light,
        ..opts(width)
    }
}

pub fn stdout(text: &str) -> Output {
    stream("stdout", text)
}

pub fn stderr(text: &str) -> Output {
    stream("stderr", text)
}

pub fn stream(name: &str, text: &str) -> Output {
    Output::new_stream(name, text)
}

/// Display data holding only `text/plain`.
pub fn plain(text: &str) -> Output {
    Output::new_display_data(
        JsonMap::from([("text/plain".to_owned(), text.into())]),
        JsonMap::new(),
    )
}

/// Display data holding the MIME bundle `data` as nbformat saves it.
///
/// The bundle goes through [`europa_core::parse`], because `serde_json` would
/// read numbers as `f64` and change their text.
pub fn display(data: &str) -> Output {
    let json = format!(r#"[{{"output_type":"display_data","data":{data},"metadata":{{}}}}]"#);
    let notebook = notebook(&json);
    cell_outputs(&notebook)[0].clone()
}

/// Execute result holding the MIME bundle `data` as nbformat saves it.
///
/// The bundle goes through [`europa_core::parse`], because `serde_json` would
/// read numbers as `f64` and change their text.
pub fn result(data: &str) -> Output {
    let json = format!(
        r#"[{{"output_type":"execute_result","data":{data},"metadata":{{}},"execution_count":1}}]"#
    );
    let notebook = notebook(&json);
    cell_outputs(&notebook)[0].clone()
}

/// Text and background of each span of the line.
pub fn code_bg(line: &StyledLine) -> Vec<(String, Option<Color>)> {
    line.spans
        .iter()
        .map(|span| (span.text.clone(), span.style.bg))
        .collect()
}

/// Check that every span holds text and neighbours differ in decoration.
pub fn assert_lines_are_well_formed(cell: &RenderedCell) {
    for (index, line) in cell.lines.iter().enumerate() {
        for span in &line.spans {
            assert!(!span.text.is_empty(), "line {index} holds an empty span");
        }
        for pair in line.spans.windows(2) {
            assert!(
                pair[0].style != pair[1].style,
                "line {index} holds adjacent spans with the same decoration"
            );
        }
    }
}

pub fn error(ename: &str, evalue: &str, traceback: &[&str]) -> Output {
    Output::new_error(
        ename,
        evalue,
        traceback.iter().map(|line| (*line).to_owned()).collect(),
    )
}

/// Outputs read from a JSON array in the shape nbformat saves them.
pub fn outputs(json: &str) -> Vec<Output> {
    serde_json::from_str(json).expect("outputs read")
}

/// The text of each display line.
pub fn texts(cell: &RenderedCell) -> Vec<String> {
    cell.lines
        .iter()
        .map(|line| line.spans.iter().map(|span| span.text.as_str()).collect())
        .collect()
}

/// The spans of a line as text and style.
pub fn styled(line: &StyledLine) -> Vec<(String, Style)> {
    line.spans
        .iter()
        .map(|span| (span.text.clone(), span.style))
        .collect()
}

/// The columns a line takes.
pub fn line_width(line: &StyledLine) -> usize {
    line.spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.text.as_str()))
        .sum()
}

/// Notebook with one code cell, `c1`, that holds the outputs in the JSON array.
pub fn notebook(outputs_json: &str) -> Notebook {
    let text = format!(
        r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{}},"cells":[{{"cell_type":"code","id":"c1","metadata":{{}},"source":"","outputs":{outputs_json},"execution_count":null}}]}}"#
    );
    europa_core::parse(text.as_bytes()).expect("fixture notebook loads")
}

/// The outputs of the cell `c1` of a notebook made by [`notebook`].
pub fn cell_outputs(notebook: &Notebook) -> &[Output] {
    let Some(Cell::Code(code)) = notebook.cells.iter().find(|cell| cell.id() == "c1") else {
        panic!("no code cell c1");
    };
    &code.outputs
}

/// What a fixture file holds.
#[derive(serde::Deserialize)]
struct Fixture {
    options: RenderOptions,
    outputs: Box<serde_json::value::RawValue>,
}

/// The `outputs` array of the file `tests/fixtures/{name}` as nbformat saves it.
pub fn fixture_outputs_json(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let fixture: Fixture =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{path}: {error}"));
    fixture.outputs.get().to_owned()
}

/// The rendering conditions and the outputs of the file `tests/fixtures/{name}`.
pub fn fixture(name: &str) -> (RenderOptions, Vec<Output>) {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let fixture: Fixture =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{path}: {error}"));
    let notebook = notebook(fixture.outputs.get());
    (fixture.options, cell_outputs(&notebook).to_vec())
}

/// Draw the cell on a test screen and describe the screen.
///
/// The screen is as wide as the render width and as high as the cell. The
/// description is the options on one line, then the buffer with the styles
/// that changed along it, so a color or a fill that stops short shows in a
/// snapshot.
pub fn draw(cell: &RenderedCell, options: &RenderOptions) -> String {
    let width = options.width.get();
    let height = u16::try_from(cell.lines.len().max(1)).expect("the cell fits a screen");
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).expect("terminal opens");
    terminal
        .draw(|frame| {
            for (row, line) in (0..).zip(&cell.lines) {
                let area = ratatui::layout::Rect::new(0, row, width, 1);
                // A line widget with no text paints nothing, not even its own
                // style, so the fill goes on the buffer before the text.
                if let Some(fill) = line.fill {
                    let style = ratatui::style::Style::new().bg(ratatui_color(fill));
                    frame.buffer_mut().set_style(area, style);
                }
                frame.render_widget(ratatui_line(line), area);
            }
        })
        .expect("draws");
    format!(
        "{}\n{:?}",
        serde_json::to_string(options).expect("serializes"),
        terminal.backend().buffer()
    )
}

/// The text of the line as ratatui draws it.
fn ratatui_line(line: &StyledLine) -> ratatui::text::Line<'static> {
    let spans: Vec<_> = line
        .spans
        .iter()
        .map(|span| ratatui::text::Span::styled(span.text.clone(), ratatui_style(span.style)))
        .collect();
    ratatui::text::Line::from(spans)
}

fn ratatui_style(style: Style) -> ratatui::style::Style {
    use ratatui::style::Modifier;

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

/// The characters of each display line.
pub fn source_lines(lines: &[StyledLine]) -> Vec<String> {
    lines
        .iter()
        .map(|line| line.spans.iter().map(|span| span.text.as_str()).collect())
        .collect()
}

/// The style of each character of the first `needle` in the display lines.
///
/// The lines are joined with `\n` before the search. Neighbouring characters
/// of one decoration share a span, so the spans cannot say where a token
/// starts; this tells the decoration character by character.
pub fn styles_of(lines: &[StyledLine], needle: &str) -> Vec<Style> {
    let mut chars: Vec<(char, Style)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            chars.push(('\n', Style::default()));
        }
        for span in &line.spans {
            chars.extend(span.text.chars().map(|c| (c, span.style)));
        }
    }
    let needle_chars: Vec<char> = needle.chars().collect();
    let found = chars.windows(needle_chars.len()).position(|window| {
        window
            .iter()
            .map(|(c, _)| *c)
            .eq(needle_chars.iter().copied())
    });
    let Some(start) = found else {
        panic!("{needle:?} not found in {:?}", source_lines(lines));
    };
    chars[start..start + needle_chars.len()]
        .iter()
        .map(|(_, style)| *style)
        .collect()
}

/// Check that the display lines of a source are well formed.
///
/// No span is empty, neighbours differ in decoration, no line has a fill and
/// no control character is left.
pub fn assert_source_lines_are_well_formed(lines: &[StyledLine]) {
    for (index, line) in lines.iter().enumerate() {
        assert!(line.fill.is_none(), "line {index} has a fill");
        for span in &line.spans {
            assert!(!span.text.is_empty(), "line {index} holds an empty span");
            assert!(
                !span.text.chars().any(char::is_control),
                "line {index} holds a control character: {:?}",
                span.text
            );
        }
        for pair in line.spans.windows(2) {
            assert!(
                pair[0].style != pair[1].style,
                "line {index} holds adjacent spans with the same decoration"
            );
        }
    }
}

/// What a source fixture file holds.
#[derive(serde::Deserialize)]
struct SourceFixture {
    options: RenderOptions,
    metadata: Box<serde_json::value::RawValue>,
    cell_type: String,
    source: Box<serde_json::value::RawValue>,
}

/// A notebook in JSON with the single cell `c1` of a source fixture file
/// `tests/fixtures/{name}`.
pub fn source_notebook_json(name: &str) -> String {
    let (_, json) = read_source_fixture(name);
    json
}

fn read_source_fixture(name: &str) -> (RenderOptions, String) {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let fixture: SourceFixture =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{path}: {error}"));
    let source = fixture.source.get();
    let cell = match fixture.cell_type.as_str() {
        "code" => format!(
            r#"{{"cell_type":"code","id":"c1","metadata":{{}},"source":{source},"outputs":[],"execution_count":null}}"#
        ),
        "raw" => format!(r#"{{"cell_type":"raw","id":"c1","metadata":{{}},"source":{source}}}"#),
        other => panic!("{path}: unsupported cell_type {other:?}"),
    };
    let json = format!(
        r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{cell}]}}"#,
        fixture.metadata.get()
    );
    (fixture.options, json)
}

/// The rendering conditions, the language and the source of the cell `c1` of
/// the file `tests/fixtures/{name}`.
pub fn source_fixture(name: &str) -> (RenderOptions, SourceLanguage, String) {
    let (options, json) = read_source_fixture(name);
    let notebook = europa_core::parse(json.as_bytes()).expect("fixture notebook loads");
    let Some(cell) = notebook.cells.iter().find(|cell| cell.id() == "c1") else {
        panic!("no cell c1");
    };
    match cell {
        Cell::Code(code) => (
            options,
            notebook.source_language(),
            code.source.text().to_owned(),
        ),
        Cell::Raw(raw) => (options, SourceLanguage::Plain, raw.source.text().to_owned()),
        _ => panic!("cell c1 is not a code or raw cell"),
    }
}

/// Draw the display lines of a source like [`draw`] does a rendered cell.
pub fn draw_lines(lines: &[StyledLine], options: &RenderOptions) -> String {
    let cell = RenderedCell {
        lines: lines.to_vec(),
        images: Vec::new(),
    };
    draw(&cell, options)
}

/// What the display lines of `source` hold when put end to end.
///
/// A tab becomes spaces up to the next stop of 8 columns, counted from the
/// start of its line, `\r\n` becomes `\n`, a control character becomes its
/// picture, and the line breaks go, because wrapping hides where they were.
pub fn expected_source_text(source: &str) -> String {
    let mut text = String::new();
    for line in source.replace("\r\n", "\n").split('\n') {
        let mut column = 0;
        for c in line.chars() {
            if c == '\t' {
                let spaces = 8 - column % 8;
                text.extend(std::iter::repeat_n(' ', spaces));
                column += spaces;
                continue;
            }
            let shown = match c {
                '\u{7f}' => '\u{2421}',
                c if (c as u32) < 0x20 => char::from_u32(0x2400 + c as u32).expect("a picture"),
                c if c.is_control() => '\u{2426}',
                c => c,
            };
            text.push(shown);
            column += unicode_width::UnicodeWidthChar::width(shown).unwrap_or(0);
        }
    }
    text
}

/// A PNG whose color follows from the coordinates.
pub fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("png encodes");
    bytes
}

/// A PNG whose pixels all have the alpha `alpha`.
pub fn png_rgba(width: u32, height: u32, alpha: u8) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([(x % 256) as u8, (y % 256) as u8, 0, alpha])
    });
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("png encodes");
    bytes
}

/// A JPEG with the same gradient as [`png`].
pub fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Jpeg,
        )
        .expect("jpeg encodes");
    bytes
}

/// A JPEG cut halfway between its start of scan and its end marker.
///
/// The image has to be big enough for that, because the cut would fail in a
/// header otherwise and never reach the check of the end marker.
pub fn jpeg_cut_in_scan(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = jpeg(width, height);
    let scan = bytes
        .windows(2)
        .rposition(|pair| pair == [0xff, 0xda])
        .expect("the encoder writes a start of scan");
    let end = bytes.len() - 2;
    assert_eq!(bytes[end..], [0xff, 0xd9]);
    assert!(end - scan > 1000, "the scan is only {} bytes", end - scan);
    bytes.truncate((scan + end) / 2);
    bytes
}

/// The color of the pixel at `x`, `y`.
pub fn rgb_at(image: &DecodedImage, x: u32, y: u32) -> [u8; 3] {
    let start = ((y * image.width + x) * 3) as usize;
    [image.rgb[start], image.rgb[start + 1], image.rgb[start + 2]]
}

/// CRC-32 as PNG chunks use it.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A PNG whose header says `width` by `height`, with data for one pixel.
///
/// Only the header matches the size asked for, which is all a size limit looks
/// at.
pub fn png_with_size(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = png(1, 1);
    bytes[16..20].copy_from_slice(&width.to_be_bytes());
    bytes[20..24].copy_from_slice(&height.to_be_bytes());
    let crc = crc32(&bytes[12..29]);
    bytes[29..33].copy_from_slice(&crc.to_be_bytes());
    bytes
}

/// A JPEG whose frame header says `width` by `height`, with data for 8 by 8.
pub fn jpeg_with_size(width: u16, height: u16) -> Vec<u8> {
    let mut bytes = jpeg(8, 8);
    let sof = bytes
        .windows(2)
        .position(|pair| pair == [0xff, 0xc0])
        .expect("the encoder writes a baseline frame header");
    bytes[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
    bytes[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
    bytes
}

/// A JPEG with an Exif block that holds the Orientation `orientation`.
pub fn jpeg_with_orientation(width: u32, height: u32, orientation: u16) -> Vec<u8> {
    let mut exif = b"Exif\0\0MM\0\x2a\0\0\0\x08\0\x01".to_vec();
    exif.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1]);
    exif.extend_from_slice(&orientation.to_be_bytes());
    exif.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    let length = u16::try_from(exif.len() + 2).expect("the block is small");
    let mut segment = vec![0xff, 0xe1];
    segment.extend_from_slice(&length.to_be_bytes());
    segment.extend_from_slice(&exif);
    let mut bytes = jpeg(width, height);
    bytes.splice(2..2, segment);
    bytes
}

/// A PNG with an `acTL` chunk after the header, which marks it animated.
pub fn apng(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = png(width, height);
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&8u32.to_be_bytes());
    chunk.extend_from_slice(b"acTL");
    chunk.extend_from_slice(&2u32.to_be_bytes());
    chunk.extend_from_slice(&0u32.to_be_bytes());
    let crc = crc32(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    bytes.splice(33..33, chunk);
    bytes
}

/// The bytes as base64, the way a notebook saves an image.
pub fn b64(bytes: &[u8]) -> String {
    base64_simd::STANDARD.encode_to_string(bytes)
}

/// Display data holding the MIME bundle `data` and the metadata `metadata`.
pub fn display_with(data: &str, metadata: &str) -> Vec<Output> {
    let json = format!(r#"[{{"output_type":"display_data","data":{data},"metadata":{metadata}}}]"#);
    cell_outputs(&notebook(&json)).to_vec()
}

/// Execute result holding the MIME bundle `data` and the metadata `metadata`.
pub fn result_with(data: &str, metadata: &str) -> Vec<Output> {
    let json = format!(
        r#"[{{"output_type":"execute_result","data":{data},"metadata":{metadata},"execution_count":1}}]"#
    );
    cell_outputs(&notebook(&json)).to_vec()
}

/// Like [`draw`], with one line per image slot after it.
///
/// The `source` of a slot is a hash, so it stays out of the snapshots.
pub fn draw_with_images(cell: &RenderedCell, options: &RenderOptions) -> String {
    let mut text = draw(cell, options);
    for slot in &cell.images {
        text.push_str(&format!(
            "\nimage line={} width={} height={} output={} mime={}",
            slot.line,
            slot.width,
            slot.height,
            slot.output,
            slot.mime.as_str()
        ));
    }
    text
}
