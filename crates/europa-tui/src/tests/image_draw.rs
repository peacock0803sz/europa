use std::num::NonZeroU16;

use europa_core::{CellPixels, DecodedImage};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui_image::picker::ProtocolType;

use super::common::{
    describe, draw_frame, first_slot, image_output_with, jpeg_output, jpeg_solid, png, png_output,
    png_solid, png_transparent, render, renderer,
};
use crate::image_cache::DEFAULT_LIMIT;
use crate::image_protocol::ColorDepth;
use crate::image_view::{fit_to_slot, nearest_256};

fn cell(width: u16, height: u16) -> CellPixels {
    CellPixels {
        width: NonZeroU16::new(width).expect("non-zero"),
        height: NonZeroU16::new(height).expect("non-zero"),
    }
}

/// Draws a whole slot on a screen that holds exactly the slot and a margin.
fn whole(
    protocol: ProtocolType,
    depth: ColorDepth,
    bytes_output: &europa_core::Output,
) -> (Buffer, u16, u16) {
    let outputs = [bytes_output.clone()];
    let mut renderer = renderer(protocol, depth, DEFAULT_LIMIT);
    let slot = first_slot(&mut renderer, &outputs);
    let area = Rect::new(0, 0, 12, slot.height + 1);
    let buffer = draw_frame(
        &mut renderer,
        &outputs,
        &slot,
        area,
        0,
        (12, slot.height + 2),
    );
    (buffer, slot.width, slot.height)
}

/// Cuts the low four bits of each channel in `Rgb(r, g, b)` of a description.
///
/// The decoder of JPEG rounds differently on different machines, and a color
/// that is the same for the whole image only differs by a few levels.
fn coarse(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("Rgb(") {
        out.push_str(&rest[..at + 4]);
        rest = &rest[at + 4..];
        let end = rest.find(')').expect("a closing bracket");
        let channels: Vec<String> = rest[..end]
            .split(", ")
            .map(|channel| (channel.parse::<u8>().expect("a channel") & 0xf0).to_string())
            .collect();
        out.push_str(&channels.join(", "));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn colors_in(buffer: &Buffer, width: u16, height: u16) -> Vec<Color> {
    let mut colors = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let cell = &buffer[(x, y)];
            colors.push(cell.fg);
            colors.push(cell.bg);
        }
    }
    colors
}

#[test]
fn a_png_is_drawn_with_half_blocks_in_true_color() {
    let (buffer, ..) = whole(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        &png_output(&png(100, 100)),
    );
    insta::assert_snapshot!("halfblock-png-rgb", describe(&buffer));
}

#[test]
fn a_jpeg_is_drawn_with_half_blocks_in_true_color() {
    let (buffer, ..) = whole(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        &jpeg_output(&jpeg_solid(100, 100)),
    );
    insta::assert_snapshot!("halfblock-jpeg-rgb", coarse(&describe(&buffer)));
}

#[test]
fn a_png_in_256_colors_keeps_no_rgb_color() {
    let (buffer, width, height) = whole(
        ProtocolType::Halfblocks,
        ColorDepth::Indexed256,
        &png_output(&png(100, 100)),
    );
    assert!(!describe(&buffer).contains("Rgb("));
    assert_eq!((width, height), (10, 5));
    insta::assert_snapshot!("halfblock-png-256", describe(&buffer));
}

#[test]
fn a_jpeg_in_256_colors_keeps_no_rgb_color() {
    let (buffer, ..) = whole(
        ProtocolType::Halfblocks,
        ColorDepth::Indexed256,
        &jpeg_output(&jpeg_solid(100, 100)),
    );
    assert!(!describe(&buffer).contains("Rgb("));
    insta::assert_snapshot!("halfblock-jpeg-256", describe(&buffer));
}

#[test]
fn a_transparent_png_is_drawn_on_white() {
    let (buffer, width, height) = whole(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        &png_output(&png_transparent(100, 100)),
    );
    let white = Color::Rgb(255, 255, 255);
    assert!(colors_in(&buffer, width, height)
        .iter()
        .all(|color| *color == white));
    insta::assert_snapshot!("halfblock-alpha", describe(&buffer));
}

#[test]
fn a_slot_that_only_partly_fits_is_drawn_only_where_it_is_visible() {
    let outputs = [png_output(&png(100, 280))];
    let mut renderer = renderer(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        DEFAULT_LIMIT,
    );
    let slot = first_slot(&mut renderer, &outputs);
    assert_eq!((slot.width, slot.height), (10, 14));
    let area = Rect::new(0, 2, 20, 5);
    let buffer = draw_frame(&mut renderer, &outputs, &slot, area, -2, (20, 10));
    for y in 0..10u16 {
        for x in 0..20u16 {
            let inside = (2..7).contains(&y) && x < 10;
            let marked = buffer[(x, y)].symbol() == "x";
            assert_eq!(marked, !inside, "cell ({x}, {y})");
        }
    }
    insta::assert_snapshot!("halfblock-partial", describe(&buffer));
}

#[test]
fn the_same_frame_again_decodes_and_encodes_nothing_more() {
    let outputs = [png_output(&png(100, 100))];
    let mut renderer = renderer(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        DEFAULT_LIMIT,
    );
    let slot = first_slot(&mut renderer, &outputs);
    let area = Rect::new(0, 0, 12, 8);
    draw_frame(&mut renderer, &outputs, &slot, area, 0, (12, 8));
    let first = renderer.stats();
    assert_eq!((first.decodes, first.encodes), (0, 1));
    draw_frame(&mut renderer, &outputs, &slot, area, 0, (12, 8));
    assert_eq!(renderer.stats(), first);
}

#[test]
fn scrolling_does_not_decode_or_encode_again() {
    let outputs = [png_output(&png(100, 280))];
    let mut renderer = renderer(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        DEFAULT_LIMIT,
    );
    let slot = first_slot(&mut renderer, &outputs);
    let area = Rect::new(0, 2, 20, 5);
    draw_frame(&mut renderer, &outputs, &slot, area, 0, (20, 10));
    let first = renderer.stats();
    for top in [-1, -2, -5, -9, 1, 3] {
        draw_frame(&mut renderer, &outputs, &slot, area, top, (20, 10));
    }
    assert_eq!(renderer.stats(), first);
}

#[test]
fn a_new_width_encodes_again_but_decodes_nothing() {
    let outputs = [png_output(&png(1000, 500))];
    let mut renderer = renderer(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        DEFAULT_LIMIT,
    );
    let narrow = render(&mut renderer, &outputs, 40, 200).images[0];
    draw_frame(
        &mut renderer,
        &outputs,
        &narrow,
        Rect::new(0, 0, 40, 30),
        0,
        (40, 30),
    );
    let before = renderer.stats();
    let wide = render(&mut renderer, &outputs, 80, 200).images[0];
    assert_ne!(narrow.width, wide.width);
    draw_frame(
        &mut renderer,
        &outputs,
        &wide,
        Rect::new(0, 0, 80, 30),
        0,
        (80, 30),
    );
    let after = renderer.stats();
    assert_eq!(after.encodes, before.encodes + 1);
    assert_eq!(after.decodes, before.decodes);
}

#[test]
fn inserting_a_source_again_keeps_what_was_encoded() {
    let outputs = [png_output(&png(100, 100))];
    let mut renderer = renderer(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        DEFAULT_LIMIT,
    );
    let slot = first_slot(&mut renderer, &outputs);
    let area = Rect::new(0, 0, 12, 8);
    draw_frame(&mut renderer, &outputs, &slot, area, 0, (12, 8));
    let before = renderer.stats();
    let again = first_slot(&mut renderer, &outputs);
    assert_eq!(again, slot);
    draw_frame(&mut renderer, &outputs, &again, area, 0, (12, 8));
    assert_eq!(renderer.stats(), before);
}

#[test]
fn the_cache_stays_within_its_limit_and_drops_the_oldest_image() {
    let outputs = [
        png_output(&png_solid(100, 100, [200, 0, 0])),
        png_output(&png_solid(100, 100, [0, 200, 0])),
        png_output(&png_solid(100, 100, [0, 0, 200])),
    ];
    // Room for one image with its encoding, which takes 90000 bytes.
    let mut renderer = renderer(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        100 * 100 * 9,
    );
    let cell = render(&mut renderer, &outputs, 80, 200);
    let area = Rect::new(0, 0, 12, 8);
    for slot in &cell.images {
        draw_frame(&mut renderer, &outputs, slot, area, 0, (12, 8));
        assert!(renderer.cached_bytes() <= 100 * 100 * 9);
    }
    let before = renderer.stats().decodes;
    draw_frame(&mut renderer, &outputs, &cell.images[0], area, 0, (12, 8));
    assert_eq!(renderer.stats().decodes, before + 1);
}

#[test]
fn one_image_over_the_limit_is_still_drawn() {
    let outputs = [png_output(&png(100, 100))];
    let mut renderer = renderer(ProtocolType::Halfblocks, ColorDepth::TrueColor, 10);
    let slot = first_slot(&mut renderer, &outputs);
    let buffer = draw_frame(
        &mut renderer,
        &outputs,
        &slot,
        Rect::new(0, 0, 12, 8),
        0,
        (12, 8),
    );
    assert_ne!(buffer[(0, 0)].symbol(), "x");
    assert!(renderer.cached_bytes() > 10);
}

#[test]
fn the_same_bytes_with_different_metadata_are_drawn_right_and_encoded_each_time() {
    let bytes = png(1000, 500);
    let outputs = [
        image_output_with("image/png", &bytes, r#"{"width":500,"height":250}"#),
        image_output_with("image/png", &bytes, r#"{"width":200,"height":100}"#),
    ];
    let mut renderer = renderer(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        DEFAULT_LIMIT,
    );
    let cell = render(&mut renderer, &outputs, 80, 200);
    let [big, small] = [cell.images[0], cell.images[1]];
    assert_eq!(big.source, small.source);
    assert_ne!((big.width, big.height), (small.width, small.height));
    let area = Rect::new(0, 0, 60, 30);
    draw_frame(&mut renderer, &outputs, &big, area, 0, (60, 30));
    let before = renderer.stats().encodes;
    for slot in [&small, &big, &small] {
        let buffer = draw_frame(&mut renderer, &outputs, slot, area, 0, (60, 30));
        assert_ne!(buffer[(0, 0)].symbol(), "x");
        assert_eq!(buffer[(slot.width, 0)].symbol(), "x", "{slot:?}");
    }
    assert_eq!(renderer.stats().encodes, before + 3);
}

#[test]
fn two_images_that_do_not_fit_together_are_drawn_right_and_decoded_each_time() {
    let outputs = [png_output(&png(100, 100)), png_output(&png(101, 100))];
    let mut renderer = renderer(ProtocolType::Halfblocks, ColorDepth::TrueColor, 10);
    let cell = render(&mut renderer, &outputs, 80, 200);
    let area = Rect::new(0, 0, 12, 8);
    draw_frame(&mut renderer, &outputs, &cell.images[0], area, 0, (12, 8));
    draw_frame(&mut renderer, &outputs, &cell.images[1], area, 0, (12, 8));
    let before = renderer.stats().decodes;
    for slot in [&cell.images[0], &cell.images[1], &cell.images[0]] {
        let buffer = draw_frame(&mut renderer, &outputs, slot, area, 0, (12, 8));
        assert_ne!(buffer[(0, 0)].symbol(), "x");
    }
    assert_eq!(renderer.stats().decodes, before + 3);
}

#[test]
fn drawing_leaves_the_saved_notebook_and_its_directory_as_they_were() {
    let bytes = png(100, 100);
    let data = base64_simd::STANDARD.encode_to_string(&bytes);
    let text = format!(
        r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{}},"cells":[{{"cell_type":"code","id":"c1","metadata":{{}},"source":"","outputs":[{{"output_type":"display_data","data":{{"image/png":"{data}"}},"metadata":{{}}}}],"execution_count":null}}]}}"#
    );
    let dir = std::env::temp_dir().join(format!("europa-image-draw-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("directory");
    let path = dir.join("book.ipynb");
    std::fs::write(&path, &text).expect("notebook");
    let notebook = europa_core::parse(&std::fs::read(&path).expect("reads")).expect("loads");
    let saved = europa_core::serialize(&notebook);
    let count = std::fs::read_dir(&dir).expect("lists").count();

    let europa_core::Cell::Code(code) = &notebook.cells[0] else {
        panic!("no code cell");
    };
    let mut renderer = renderer(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        DEFAULT_LIMIT,
    );
    let slot = first_slot(&mut renderer, &code.outputs);
    draw_frame(
        &mut renderer,
        &code.outputs,
        &slot,
        Rect::new(0, 0, 12, 8),
        0,
        (12, 8),
    );

    assert_eq!(europa_core::serialize(&notebook), saved);
    assert_eq!(std::fs::read_dir(&dir).expect("lists").count(), count);
    std::fs::remove_dir_all(&dir).expect("removes");
}

fn decoded(width: u32, height: u32) -> DecodedImage {
    let mut rgb = Vec::new();
    for y in 0..height {
        for x in 0..width {
            rgb.extend_from_slice(&[(x % 200) as u8, (y % 200) as u8, 100]);
        }
    }
    DecodedImage { width, height, rgb }
}

#[test]
fn an_image_that_matches_its_slot_is_placed_unchanged_at_the_top_left() {
    let image = fit_to_slot(&decoded(645, 485), 65, 25, CellPixels::DEFAULT).to_rgb8();
    assert_eq!(image.dimensions(), (650, 500));
    assert_eq!(image.get_pixel(0, 0).0, [0, 0, 100]);
    assert_eq!(
        image.get_pixel(644, 484).0,
        [(644 % 200) as u8, (484 % 200) as u8, 100]
    );
    assert_eq!(image.get_pixel(649, 499).0, [255, 255, 255]);
}

#[test]
fn a_small_image_is_not_enlarged() {
    let image = fit_to_slot(&decoded(15, 15), 2, 1, CellPixels::DEFAULT).to_rgb8();
    assert_eq!(image.dimensions(), (20, 20));
    assert_eq!(image.get_pixel(14, 14).0, [14, 14, 100]);
    assert_eq!(image.get_pixel(15, 0).0, [255, 255, 255]);
    assert_eq!(image.get_pixel(0, 15).0, [255, 255, 255]);
}

#[test]
fn an_image_is_scaled_to_fit_a_slot_of_another_size() {
    let enlarged = fit_to_slot(&decoded(100, 100), 40, 20, CellPixels::DEFAULT).to_rgb8();
    assert_eq!(enlarged.dimensions(), (400, 400));
    assert_ne!(enlarged.get_pixel(399, 399).0, [255, 255, 255]);
    let shrunk = fit_to_slot(&decoded(1280, 960), 64, 24, CellPixels::DEFAULT).to_rgb8();
    assert_eq!(shrunk.dimensions(), (640, 480));
    assert_ne!(shrunk.get_pixel(639, 479).0, [255, 255, 255]);
}

#[test]
fn colors_go_to_the_nearest_of_the_256() {
    assert_eq!(nearest_256(255, 0, 0), 196);
    assert_eq!(nearest_256(128, 128, 128), 244);
    assert_eq!(nearest_256(215, 0, 0), 160);
    assert_eq!(nearest_256(95, 95, 95), 59);
}

#[test]
fn a_new_cell_size_from_the_window_is_taken_once() {
    let mut renderer = renderer(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        DEFAULT_LIMIT,
    );
    assert!(!renderer.apply_window(None));
    assert!(!renderer.apply_window(Some(CellPixels::DEFAULT)));
    assert_eq!(renderer.cell_pixels(), CellPixels::DEFAULT);
    assert!(renderer.apply_window(Some(cell(8, 16))));
    assert_eq!(renderer.cell_pixels(), cell(8, 16));
    assert!(!renderer.apply_window(Some(cell(8, 16))));
}
