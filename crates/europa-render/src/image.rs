//! Reading images from outputs: base64, decoding and the size of the room
//! an image takes in a rendered cell.

use std::io::Cursor;

use europa_core::{DecodedImage, JsonValue, RenderOptions};
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader, Limits};

/// Most pixels an image may have, which is 8000 by 8000.
const MAX_PIXELS: u64 = 64_000_000;

/// Most memory the decoder may allocate, in bytes.
const MAX_ALLOC: u64 = 1 << 30;

/// Why an image could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageFailure {
    /// The data is not a PNG or a JPEG that decodes to the end.
    Broken,
    /// The image has more pixels than [`MAX_PIXELS`] or needs too much memory.
    TooLarge,
}

/// The bytes that a notebook value holds as base64.
///
/// A value is a string or an array of strings, which are joined as nbformat
/// does for multiline text. Whitespace between the characters is ignored.
pub(crate) fn read_value(value: &JsonValue) -> Result<Vec<u8>, ImageFailure> {
    let text = match value {
        JsonValue::String(text) => text.clone(),
        JsonValue::Array(parts) => {
            let mut text = String::new();
            for part in parts {
                text.push_str(part.as_str().ok_or(ImageFailure::Broken)?);
            }
            text
        }
        _ => return Err(ImageFailure::Broken),
    };
    base64_simd::forgiving_decode_to_vec(text.as_bytes()).map_err(|_| ImageFailure::Broken)
}

/// Decodes a PNG or a JPEG to 8-bit RGB, with transparency put on white.
///
/// The format comes from the content, not from the name of the MIME type.
pub(crate) fn decode(bytes: &[u8]) -> Result<DecodedImage, ImageFailure> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ImageFailure::Broken)?;
    match reader.format() {
        Some(ImageFormat::Png) => {}
        Some(ImageFormat::Jpeg) if jpeg_is_complete(bytes) => {}
        _ => return Err(ImageFailure::Broken),
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(classify)?;
    // The size is known from the header, so a huge image is refused before
    // its pixels are allocated.
    let (width, height) = decoder.dimensions();
    if exceeds_pixel_limit(width, height) {
        return Err(ImageFailure::TooLarge);
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(classify)?;
    image.apply_orientation(orientation);

    let (width, height) = (image.width(), image.height());
    // The image is consumed, because keeping it through a conversion holds a
    // second copy of the pixels at the peak of memory.
    let rgb = if image.color().has_alpha() {
        let rgba = image.into_rgba8();
        let mut rgb = Vec::with_capacity(rgba.as_raw().len() / 4 * 3);
        for pixel in rgba.pixels() {
            let alpha = u32::from(pixel[3]);
            for channel in &pixel.0[..3] {
                let value = (u32::from(*channel) * alpha + 255 * (255 - alpha) + 127) / 255;
                rgb.push(u8::try_from(value).expect("a weighted mean of bytes is a byte"));
            }
        }
        rgb
    } else {
        image.into_rgb8().into_raw()
    };
    Ok(DecodedImage { width, height, rgb })
}

/// Tells a refusal for size from a failure to read.
fn classify(error: ImageError) -> ImageFailure {
    match error {
        ImageError::Limits(_) => ImageFailure::TooLarge,
        _ => ImageFailure::Broken,
    }
}

fn exceeds_pixel_limit(width: u32, height: u32) -> bool {
    u64::from(width) * u64::from(height) > MAX_PIXELS
}

/// Whether the JPEG ends the way a complete one does.
///
/// The decoder reads a JPEG that was cut short and fills what is missing
/// with gray, so the end marker is checked here. A data byte `FF` in a scan
/// is written as `FF 00` and the only markers inside a scan are restarts, so
/// `FF D9` after a start of scan is a real end marker. Searching from the
/// last `FF DA` skips the markers of an Exif thumbnail, which come before
/// the scan of the image. Trailing bytes that hold `FF DA` make a complete
/// JPEG count as cut short.
fn jpeg_is_complete(bytes: &[u8]) -> bool {
    let Some(scan) = bytes.windows(2).rposition(|pair| pair == [0xff, 0xda]) else {
        return false;
    };
    bytes[scan..].windows(2).any(|pair| pair == [0xff, 0xd9])
}

/// FNV-1a, 64 bits, of the bytes after base64 decoding. A key for caching
/// decoded images.
pub(crate) fn source_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// A positive, finite number at `key` of the metadata of an image.
fn positive(metadata: &JsonValue, key: &str) -> Option<f64> {
    let number = metadata.as_object()?.get(key)?.as_number()?.as_f64();
    (number.is_finite() && number > 0.0).then_some(number)
}

/// The size an image asks to be shown at, in pixels.
///
/// `metadata` is the object that the output keeps for the MIME type. When
/// only one of `width` and `height` is usable, the other follows from the
/// aspect ratio of the pixels. Anything unusable falls back to the pixels.
pub(crate) fn natural_size(metadata: Option<&JsonValue>, pixels: (u32, u32)) -> (f64, f64) {
    let (pixel_width, pixel_height) = (f64::from(pixels.0), f64::from(pixels.1));
    let Some(metadata) = metadata else {
        return (pixel_width, pixel_height);
    };
    let derived = match (positive(metadata, "width"), positive(metadata, "height")) {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (width, width * pixel_height / pixel_width),
        (None, Some(height)) => (height * pixel_width / pixel_height, height),
        (None, None) => return (pixel_width, pixel_height),
    };
    if derived.0.is_finite() && derived.0 > 0.0 && derived.1.is_finite() && derived.1 > 0.0 {
        derived
    } else {
        (pixel_width, pixel_height)
    }
}

/// Columns and lines to keep for an image of `natural` pixels.
///
/// The image is shrunk to fit the width and the line limit, never enlarged.
pub(crate) fn slot_size(natural: (f64, f64), options: &RenderOptions) -> (u16, u16) {
    let columns = f64::from(options.width.get());
    let limit = options.max_output_lines.get() as f64;
    let cell_width = f64::from(options.cell_pixels.width.get());
    let cell_height = f64::from(options.cell_pixels.height.get());
    let (width, height) = natural;
    let scale = 1.0_f64
        .min(columns * cell_width / width)
        .min(limit * cell_height / height);
    let max_height = limit.min(f64::from(u16::MAX));
    (
        fit(width * scale / cell_width, columns),
        fit(height * scale / cell_height, max_height),
    )
}

/// Rounds up to whole cells, keeping the result between 1 and `most`.
///
/// The small subtraction stops a value that floating point made a hair too
/// large from taking one more cell. `f64::clamp` would pass NaN on.
fn fit(cells: f64, most: f64) -> u16 {
    let rounded = (cells - 1e-9).ceil();
    let rounded = if rounded.is_nan() { 1.0 } else { rounded };
    // Between 1 and a u16 value, so the cast cannot lose anything.
    rounded.max(1.0).min(most) as u16
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU16, NonZeroUsize};

    use europa_core::{CellPixels, JsonMap};

    use super::*;

    fn gradient() -> image::RgbImage {
        image::RgbImage::from_fn(16, 12, |x, y| {
            image::Rgb([(x * 10) as u8, (y * 10) as u8, 7])
        })
    }

    fn encode(image: impl Into<DynamicImage>, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Vec::new();
        image
            .into()
            .write_to(&mut Cursor::new(&mut bytes), format)
            .expect("encodes");
        bytes
    }

    fn png() -> Vec<u8> {
        encode(gradient(), ImageFormat::Png)
    }

    fn jpeg() -> Vec<u8> {
        encode(gradient(), ImageFormat::Jpeg)
    }

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

    fn png_with_size(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = png();
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        let crc = crc32(&bytes[12..29]);
        bytes[29..33].copy_from_slice(&crc.to_be_bytes());
        bytes
    }

    fn jpeg_with_size(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = jpeg();
        let sof = bytes
            .windows(2)
            .position(|pair| pair == [0xff, 0xc0])
            .expect("baseline frame header");
        bytes[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
        bytes[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
        bytes
    }

    fn orientation_6(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::new(width, height);
        let mut bytes = encode(image, ImageFormat::Jpeg);
        let mut exif = b"Exif\0\0MM\0\x2a\0\0\0\x08\0\x01".to_vec();
        exif.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, 0, 0, 0, 0]);
        let length = u16::try_from(exif.len() + 2).expect("small");
        let mut segment = vec![0xff, 0xe1];
        segment.extend_from_slice(&length.to_be_bytes());
        segment.extend_from_slice(&exif);
        bytes.splice(2..2, segment);
        bytes
    }

    fn options(width: u16, max: usize, cell: (u16, u16)) -> RenderOptions {
        RenderOptions {
            max_output_lines: NonZeroUsize::new(max).expect("non-zero"),
            cell_pixels: CellPixels {
                width: NonZeroU16::new(cell.0).expect("non-zero"),
                height: NonZeroU16::new(cell.1).expect("non-zero"),
            },
            ..RenderOptions::new(NonZeroU16::new(width).expect("non-zero"))
        }
    }

    fn metadata(json: &str) -> JsonValue {
        let notebook =
            format!(r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{"x":{json}}},"cells":[]}}"#);
        let notebook = europa_core::parse(notebook.as_bytes()).expect("loads");
        notebook.metadata["x"].clone()
    }

    #[test]
    fn base64_with_line_breaks_and_spaces_is_read() {
        let value = JsonValue::String("aGVs\nbG8g\n d29ybGQ=".to_owned());
        assert_eq!(read_value(&value).expect("reads"), b"hello world");
    }

    #[test]
    fn the_format_comes_from_the_content() {
        let reader = ImageReader::new(Cursor::new(jpeg()))
            .with_guessed_format()
            .expect("guesses");
        assert_eq!(reader.format(), Some(ImageFormat::Jpeg));
    }

    #[test]
    fn the_size_of_a_huge_png_is_known_without_allocating() {
        let bytes = png_with_size(60000, 60000);
        let decoder = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .expect("guesses")
            .into_decoder()
            .expect("header reads");
        assert_eq!(decoder.dimensions(), (60000, 60000));
    }

    #[test]
    fn an_orientation_of_6_swaps_width_and_height() {
        let mut decoder = ImageReader::new(Cursor::new(orientation_6(40, 30)))
            .with_guessed_format()
            .expect("guesses")
            .into_decoder()
            .expect("header reads");
        let orientation = decoder.orientation().expect("orientation reads");
        let mut image = DynamicImage::from_decoder(decoder).expect("decodes");
        image.apply_orientation(orientation);
        assert_eq!((image.width(), image.height()), (30, 40));
    }

    #[test]
    fn values_that_are_not_strings_are_broken() {
        let array = JsonValue::Array(vec!["aGVs".into(), "bG8=".into()]);
        assert_eq!(read_value(&array).expect("reads"), b"hello");
        assert_eq!(read_value(&JsonValue::Null), Err(ImageFailure::Broken));
        assert_eq!(
            read_value(&JsonValue::String("!!!".to_owned())),
            Err(ImageFailure::Broken)
        );
        let mixed = JsonValue::Array(vec!["aGVs".into(), JsonValue::Null]);
        assert_eq!(read_value(&mixed), Err(ImageFailure::Broken));
    }

    #[test]
    fn png_and_jpeg_decode_to_rgb() {
        for bytes in [png(), jpeg()] {
            let image = decode(&bytes).expect("decodes");
            assert_eq!((image.width, image.height), (16, 12));
            assert_eq!(image.rgb.len(), 16 * 12 * 3);
        }
    }

    fn rgb_at(image: &DecodedImage, x: u32, y: u32) -> [u8; 3] {
        let start = ((y * image.width + x) * 3) as usize;
        [image.rgb[start], image.rgb[start + 1], image.rgb[start + 2]]
    }

    #[test]
    fn a_png_keeps_the_exact_color_of_each_pixel() {
        let image = decode(&png()).expect("decodes");
        for (x, y, expected) in [
            (0, 0, [0, 0, 7]),
            (15, 0, [150, 0, 7]),
            (0, 11, [0, 110, 7]),
            (3, 7, [30, 70, 7]),
            (15, 11, [150, 110, 7]),
        ] {
            assert_eq!(rgb_at(&image, x, y), expected, "at {x},{y}");
        }
    }

    #[test]
    fn a_jpeg_keeps_the_color_of_each_block_within_a_tolerance() {
        // Blocks line up with the 16 by 16 units of the encoder, so the middle
        // of a block takes no color from its neighbours.
        let top_left = [220, 30, 30];
        let top_right = [30, 220, 30];
        let bottom_left = [30, 30, 220];
        let bottom_right = [220, 220, 30];
        let blocks = image::RgbImage::from_fn(32, 32, |x, y| {
            image::Rgb(match (x < 16, y < 16) {
                (true, true) => top_left,
                (false, true) => top_right,
                (true, false) => bottom_left,
                (false, false) => bottom_right,
            })
        });
        let image = decode(&encode(blocks, ImageFormat::Jpeg)).expect("decodes");
        for (x, y, expected) in [
            (8, 8, top_left),
            (24, 8, top_right),
            (8, 24, bottom_left),
            (24, 24, bottom_right),
        ] {
            let actual = rgb_at(&image, x, y);
            for (got, want) in actual.into_iter().zip(expected) {
                assert!(
                    got.abs_diff(want) <= 16,
                    "at {x},{y}: {actual:?} is not near {expected:?}"
                );
            }
        }
    }

    #[test]
    fn broken_data_is_refused() {
        let png = png();
        let jpeg = jpeg();
        assert_eq!(decode(&png[..png.len() / 2]), Err(ImageFailure::Broken));
        assert_eq!(decode(&jpeg[..jpeg.len() / 2]), Err(ImageFailure::Broken));
        let gif = b"GIF89a\x01\0\x01\0\0\0\0;";
        assert_eq!(decode(gif), Err(ImageFailure::Broken));
        assert_eq!(decode(b"not an image"), Err(ImageFailure::Broken));
    }

    #[test]
    fn a_jpeg_cut_in_its_scan_is_refused() {
        // The cut has to land in the scan, because one in a header fails there
        // and never reaches the check of the end marker.
        let image = image::RgbImage::from_fn(128, 128, |x, y| {
            image::Rgb([(x * 2) as u8, (y * 2) as u8, (x * y % 256) as u8])
        });
        let bytes = encode(image, ImageFormat::Jpeg);
        let scan = bytes
            .windows(2)
            .rposition(|pair| pair == [0xff, 0xda])
            .expect("start of scan");
        let end = bytes.len() - 2;
        assert_eq!(bytes[end..], [0xff, 0xd9]);
        assert!(end - scan > 1000, "the scan is only {} bytes", end - scan);
        let cut = &bytes[..(scan + end) / 2];
        assert!(!jpeg_is_complete(cut));
        assert_eq!(decode(cut), Err(ImageFailure::Broken));
    }

    #[test]
    fn bytes_after_the_end_of_a_jpeg_are_ignored() {
        let mut bytes = jpeg();
        bytes.extend_from_slice(b"trailing");
        assert!(decode(&bytes).is_ok());
    }

    #[test]
    fn too_large_images_are_refused_by_their_header() {
        for bytes in [
            png_with_size(8001, 8001),
            png_with_size(60000, 60000),
            png_with_size(2_147_483_647, 1),
            jpeg_with_size(8001, 8001),
        ] {
            assert_eq!(decode(&bytes), Err(ImageFailure::TooLarge));
        }
    }

    #[test]
    fn transparency_is_put_on_white() {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 0]));
        let decoded = decode(&encode(image, ImageFormat::Png)).expect("decodes");
        assert!(decoded.rgb.chunks(3).all(|pixel| pixel == [255, 255, 255]));
    }

    #[test]
    fn the_pixel_limit_is_8000_by_8000() {
        assert!(!exceeds_pixel_limit(8000, 8000));
        assert!(exceeds_pixel_limit(8001, 8000));
    }

    #[test]
    fn a_jpeg_is_complete_when_it_has_its_end() {
        let jpeg = jpeg();
        assert!(jpeg_is_complete(&jpeg));
        assert!(!jpeg_is_complete(&jpeg[..jpeg.len() / 2]));
        let mut padded = jpeg;
        padded.extend_from_slice(b"junk");
        assert!(jpeg_is_complete(&padded));
    }

    #[test]
    fn the_hash_is_fnv_1a() {
        assert_eq!(source_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(source_hash(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    fn slot(metadata_json: Option<&str>, pixels: (u32, u32), width: u16, max: usize) -> (u16, u16) {
        let metadata = metadata_json.map(metadata);
        let natural = natural_size(metadata.as_ref(), pixels);
        slot_size(natural, &options(width, max, (10, 20)))
    }

    #[test]
    fn slot_sizes_follow_the_contract_table() {
        let none = None;
        assert_eq!(slot(none, (640, 480), 80, 200), (64, 24));
        assert_eq!(slot(none, (640, 480), 32, 200), (32, 12));
        let both = Some(r#"{"width":640,"height":480}"#);
        assert_eq!(slot(both, (1280, 960), 80, 200), (64, 24));
        assert_eq!(
            slot(Some(r#"{"width":640}"#), (1280, 960), 80, 200),
            (64, 24)
        );
        assert_eq!(
            slot(Some(r#"{"height":480}"#), (1280, 960), 80, 200),
            (64, 24)
        );
        assert_eq!(
            slot(Some(r#"{"width":0,"height":"a"}"#), (1280, 960), 80, 200),
            (80, 30)
        );
        assert_eq!(
            slot(Some(r#"{"width":-1,"height":-1}"#), (1280, 960), 80, 200),
            (80, 30)
        );
        assert_eq!(
            slot(Some(r#"{"width":1e308}"#), (100, 200), 80, 200),
            (10, 10)
        );
        assert_eq!(slot(none, (100, 8000), 80, 200), (5, 200));
        assert_eq!(slot(none, (100, 400), 80, 7), (4, 7));
        assert_eq!(slot(none, (1060, 1060), 80, 200), (80, 40));
        assert_eq!(slot(none, (15, 15), 80, 200), (2, 1));
        assert_eq!(slot(none, (3, 3), 1, 200), (1, 1));
        assert_eq!(slot(none, (640, 480), 1, 1), (1, 1));
    }

    #[test]
    fn slot_sizes_follow_the_cell_size() {
        let natural = natural_size(None, (640, 480));
        assert_eq!(slot_size(natural, &options(80, 200, (20, 40))), (32, 12));
    }

    #[test]
    fn metadata_without_an_object_is_ignored() {
        let value = JsonValue::Object(JsonMap::new());
        assert_eq!(natural_size(Some(&value), (7, 9)), (7.0, 9.0));
        assert_eq!(natural_size(Some(&JsonValue::Null), (7, 9)), (7.0, 9.0));
    }
}
