use std::num::NonZeroU16;

use europa_core::{CellPixels, DecodedImage};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui_image::picker::ProtocolType;

use super::common::{
    describe, describe_protocol_frame, draw_frame, first_slot, image_output_with, jpeg_output,
    jpeg_solid, png, png_output, png_solid, png_transparent, render, renderer,
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

/// A cell of a line of text, an image and a line of text, with the renderer
/// that has the pixels of its image.
fn text_and_image() -> (
    crate::image_view::ImageRenderer,
    Vec<europa_core::Output>,
    europa_core::RenderedCell,
) {
    let outputs = vec![
        super::common::stdout("before\n"),
        png_output(&png(100, 60)),
        super::common::stdout("after\n"),
    ];
    let mut renderer = renderer(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        DEFAULT_LIMIT,
    );
    let cell = render(&mut renderer, &outputs, 20, 200);
    (renderer, outputs, cell)
}

fn marked_screen(width: u16, height: u16) -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    for cell in &mut buffer.content {
        cell.set_symbol("x");
    }
    buffer
}

#[test]
fn text_and_an_image_are_drawn_in_one_cell() {
    let (mut renderer, outputs, cell) = text_and_image();
    let area = Rect::new(0, 0, 20, 5);
    let mut buffer = marked_screen(20, 6);
    crate::cell_view::draw_cell(&mut buffer, area, &cell, &outputs, 0, &mut renderer);
    insta::assert_snapshot!("halfblock-with-text", describe(&buffer));
}

#[test]
fn what_is_outside_the_slots_is_the_same_with_or_without_the_images() {
    let (mut renderer, outputs, cell) = text_and_image();
    let area = Rect::new(0, 0, 20, 5);
    let mut with = marked_screen(20, 6);
    crate::cell_view::draw_cell(&mut with, area, &cell, &outputs, 0, &mut renderer);
    let mut without = marked_screen(20, 6);
    let bare = europa_core::RenderedCell {
        images: Vec::new(),
        ..cell.clone()
    };
    crate::cell_view::draw_cell(&mut without, area, &bare, &outputs, 0, &mut renderer);

    let slot = cell.images[0];
    let mut differing = 0;
    for y in 0..6u16 {
        for x in 0..20u16 {
            let inside = (1..4).contains(&y) && x < slot.width;
            if inside {
                differing += usize::from(with[(x, y)] != without[(x, y)]);
            } else {
                assert_eq!(with[(x, y)], without[(x, y)], "cell ({x}, {y})");
            }
        }
    }
    assert!(differing > 0, "the image was not drawn");
}

#[test]
fn lines_scrolled_out_of_the_area_are_not_drawn() {
    let (mut renderer, outputs, cell) = text_and_image();
    let area = Rect::new(0, 0, 20, 2);
    let mut buffer = marked_screen(20, 3);
    crate::cell_view::draw_cell(&mut buffer, area, &cell, &outputs, 3, &mut renderer);
    let text: String = (0..20)
        .map(|x| buffer[(x, 1)].symbol().to_owned())
        .collect();
    assert!(text.starts_with("after"), "{text:?}");
    assert_eq!(buffer[(0, 2)].symbol(), "x");
}

#[test]
fn the_visible_placements_follow_the_scroll() {
    let (renderer, _, cell) = text_and_image();
    let source = cell.images[0].source;
    let area = Rect::new(2, 3, 20, 2);
    let placement = |y, height| crate::image_view::Placement {
        source,
        x: 2,
        y,
        width: 10,
        height,
    };
    let at = |scroll| crate::cell_view::visible_placements(area, &cell, scroll);
    // The slot is the lines 1 to 3 of the cell, and the area shows two lines.
    assert_eq!(at(0), vec![placement(4, 1)]);
    assert_eq!(at(1), vec![placement(3, 2)]);
    assert_eq!(at(2), vec![placement(3, 2)]);
    assert_eq!(at(3), vec![placement(3, 1)]);
    assert_eq!(at(4), vec![]);
    drop(renderer);
}

const PROTOCOLS: [ProtocolType; 4] = [
    ProtocolType::Kitty,
    ProtocolType::Sixel,
    ProtocolType::Iterm2,
    ProtocolType::Halfblocks,
];

/// How a slot of 10 columns and 14 lines is cut by an area of 5 lines at
/// `top`: the first and the last line that show, and how many show.
const CUTS: [(i32, usize, usize); 6] = [
    (0, 0, 5),
    (3, 0, 2),
    (-12, 12, 2),
    (-2, 2, 5),
    (5, 0, 0),
    (-14, 0, 0),
];

fn tall() -> (Vec<europa_core::Output>, Vec<u8>) {
    (vec![png_output(&png(100, 280))], Vec::new())
}

/// The frame of `top` on a screen of 20 by 10 with an area of 5 lines.
fn cut_frame(protocol: ProtocolType, top: i32) -> Buffer {
    let (outputs, _) = tall();
    let mut renderer = renderer(protocol, ColorDepth::TrueColor, DEFAULT_LIMIT);
    let slot = first_slot(&mut renderer, &outputs);
    draw_frame(
        &mut renderer,
        &outputs,
        &slot,
        Rect::new(0, 2, 20, 5),
        top,
        (20, 10),
    )
}

/// The frame of the whole slot, on a screen that holds all of it.
fn whole_frame(protocol: ProtocolType) -> Buffer {
    let (outputs, _) = tall();
    let mut renderer = renderer(protocol, ColorDepth::TrueColor, DEFAULT_LIMIT);
    let slot = first_slot(&mut renderer, &outputs);
    draw_frame(
        &mut renderer,
        &outputs,
        &slot,
        Rect::new(0, 2, 20, 14),
        0,
        (20, 18),
    )
}

#[test]
fn nothing_outside_the_visible_part_changes_whatever_the_protocol() {
    for protocol in PROTOCOLS {
        for (top, first, shown) in CUTS {
            let buffer = cut_frame(protocol, top);
            let marked = marked_screen(20, 10);
            for y in 0..10u16 {
                for x in 0..20u16 {
                    let from = 2 + top.max(0) as u16;
                    let visible = x < 10 && y >= from && usize::from(y - from) < shown;
                    if !visible {
                        assert_eq!(
                            buffer[(x, y)],
                            marked[(x, y)],
                            "{protocol:?}, top {top}, cell ({x}, {y})"
                        );
                    }
                }
            }
            let _ = first;
        }
    }
}

#[test]
fn a_slot_that_is_out_of_the_area_draws_nothing() {
    for protocol in PROTOCOLS {
        for top in [5, -14] {
            assert_eq!(
                cut_frame(protocol, top),
                marked_screen(20, 10),
                "{protocol:?}, {top}"
            );
        }
    }
}

#[test]
fn half_blocks_leave_out_only_the_lines_that_are_cut() {
    let whole = whole_frame(ProtocolType::Halfblocks);
    for (top, first, shown) in CUTS {
        let buffer = cut_frame(ProtocolType::Halfblocks, top);
        for line in 0..shown {
            for x in 0..10u16 {
                let shown_at = 2 + top.max(0) as u16 + u16::try_from(line).expect("small");
                let seen = &buffer[(x, shown_at)];
                let all = &whole[(x, 2 + u16::try_from(first + line).expect("small"))];
                assert_eq!(seen, all, "top {top}, line {line}, column {x}");
            }
        }
    }
}

#[test]
fn sixel_keeps_the_bands_of_the_visible_lines_only() {
    for (top, _, shown) in CUTS {
        let text = describe_protocol_frame(&cut_frame(ProtocolType::Sixel, top));
        if shown == 0 {
            assert!(!text.contains("sixel bands"), "top {top}: {text}");
            continue;
        }
        let bands: usize = text
            .lines()
            .find_map(|line| line.strip_prefix("sixel bands ["))
            .and_then(|rest| rest.trim_end_matches(']').parse().ok())
            .unwrap_or_else(|| panic!("top {top}: {text}"));
        let rows = shown * 20;
        assert!(
            (rows - 20) < bands * 6 && bands * 6 <= rows,
            "top {top}: {bands} bands for {shown} lines"
        );
    }
}

#[test]
fn iterm2_has_one_image_per_visible_line() {
    for (top, _, shown) in CUTS {
        let text = describe_protocol_frame(&cut_frame(ProtocolType::Iterm2, top));
        let count = text.chars().filter(|c| *c == 'I').count();
        assert_eq!(count, shown, "top {top}\n{text}");
        for (y, line) in text.lines().enumerate() {
            let from = 2 + top.max(0) as usize;
            let expected = usize::from((from..from + shown).contains(&y));
            assert_eq!(
                line.chars().filter(|c| *c == 'I').count(),
                expected,
                "top {top}\n{text}"
            );
        }
    }
}

#[test]
fn kitty_covers_the_visible_part_and_numbers_the_lines_of_the_slot() {
    let whole = describe_protocol_frame(&whole_frame(ProtocolType::Kitty));
    let all_rows: Vec<&str> = whole
        .lines()
        .find_map(|line| line.strip_prefix("kitty rows "))
        .expect("rows")
        .split(' ')
        .collect();
    for (top, first, shown) in CUTS {
        let text = describe_protocol_frame(&cut_frame(ProtocolType::Kitty, top));
        let placeholders = text.chars().filter(|c| *c == 'K').count();
        assert_eq!(placeholders, 10 * shown, "top {top}\n{text}");
        if shown == 0 {
            continue;
        }
        let rows: Vec<&str> = text
            .lines()
            .find_map(|line| line.strip_prefix("kitty rows "))
            .expect("rows")
            .split(' ')
            .collect();
        assert_eq!(rows, all_rows[first..first + shown], "top {top}");
    }
}

#[test]
fn a_slot_of_one_line_area_is_drawn_by_every_protocol() {
    for protocol in PROTOCOLS {
        let (outputs, _) = tall();
        let mut renderer = renderer(protocol, ColorDepth::TrueColor, DEFAULT_LIMIT);
        let slot = first_slot(&mut renderer, &outputs);
        let area = Rect::new(0, 4, 20, 1);
        let buffer = draw_frame(&mut renderer, &outputs, &slot, area, -5, (20, 10));
        let marked = marked_screen(20, 10);
        for y in (0..10u16).filter(|y| *y != 4) {
            for x in 0..20u16 {
                assert_eq!(buffer[(x, y)], marked[(x, y)], "{protocol:?} ({x}, {y})");
            }
        }
    }
}

#[test]
fn an_area_narrower_than_the_slot_does_not_panic_or_spill() {
    for protocol in PROTOCOLS {
        let (outputs, _) = tall();
        let mut renderer = renderer(protocol, ColorDepth::TrueColor, DEFAULT_LIMIT);
        let slot = first_slot(&mut renderer, &outputs);
        let area = Rect::new(0, 2, 6, 5);
        for top in [0, -2, 3] {
            let buffer = draw_frame(&mut renderer, &outputs, &slot, area, top, (20, 10));
            let marked = marked_screen(20, 10);
            for y in 0..10u16 {
                for x in 0..20u16 {
                    let inside = x < 6 && (2..7).contains(&y);
                    if !inside {
                        assert_eq!(
                            buffer[(x, y)],
                            marked[(x, y)],
                            "{protocol:?}, top {top}, ({x}, {y})"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn the_frames_of_each_protocol_are_pinned() {
    for (protocol, name) in [
        (ProtocolType::Kitty, "protocol-kitty"),
        (ProtocolType::Sixel, "protocol-sixel"),
        (ProtocolType::Iterm2, "protocol-iterm2"),
    ] {
        let frames: Vec<String> = [-2, -12, 3, -14]
            .iter()
            .map(|top| {
                format!(
                    "top {top}\n{}",
                    describe_protocol_frame(&cut_frame(protocol, *top))
                )
            })
            .collect();
        insta::assert_snapshot!(name, frames.join("\n"));
    }
}

#[test]
fn kitty_draws_a_slot_of_more_than_297_lines_as_half_blocks() {
    let outputs = [png_output(&png(100, 6000))];
    let mut kitty = renderer(ProtocolType::Kitty, ColorDepth::TrueColor, DEFAULT_LIMIT);
    let slot = render(&mut kitty, &outputs, 80, 300).images[0];
    assert!(slot.height > 297);
    let area = Rect::new(0, 0, 20, 8);
    let drawn = draw_frame(&mut kitty, &outputs, &slot, area, 0, (20, 8));
    let mut blocks = renderer(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        DEFAULT_LIMIT,
    );
    let slot = render(&mut blocks, &outputs, 80, 300).images[0];
    let expected = draw_frame(&mut blocks, &outputs, &slot, area, 0, (20, 8));
    assert!(!describe_protocol_frame(&drawn).contains('K'));
    assert_eq!(drawn, expected);
}

#[test]
fn kitty_draws_a_slot_of_more_than_297_columns_as_half_blocks() {
    let outputs = [png_output(&png(3000, 20))];
    let mut kitty = renderer(ProtocolType::Kitty, ColorDepth::TrueColor, DEFAULT_LIMIT);
    let slot = render(&mut kitty, &outputs, 300, 200).images[0];
    assert!(slot.width > 297);
    let area = Rect::new(0, 0, 300, 2);
    let drawn = draw_frame(&mut kitty, &outputs, &slot, area, 0, (300, 2));
    let mut blocks = renderer(
        ProtocolType::Halfblocks,
        ColorDepth::TrueColor,
        DEFAULT_LIMIT,
    );
    let slot = render(&mut blocks, &outputs, 300, 200).images[0];
    let expected = draw_frame(&mut blocks, &outputs, &slot, area, 0, (300, 2));
    assert!(!describe_protocol_frame(&drawn).contains('K'));
    assert_eq!(drawn, expected);
}

fn placement(source: u64, y: u16) -> crate::image_view::Placement {
    crate::image_view::Placement {
        source,
        x: 0,
        y,
        width: 10,
        height: 3,
    }
}

#[test]
fn sixel_and_iterm2_ask_for_a_clear_when_the_placements_differ() {
    for protocol in [ProtocolType::Sixel, ProtocolType::Iterm2] {
        let mut renderer = renderer(protocol, ColorDepth::TrueColor, DEFAULT_LIMIT);
        assert!(
            !renderer.placements_changed(&[]),
            "{protocol:?} nothing at first"
        );
        assert!(
            renderer.placements_changed(&[placement(1, 0)]),
            "{protocol:?} a new image"
        );
        assert!(
            !renderer.placements_changed(&[placement(1, 0)]),
            "{protocol:?} the same"
        );
        assert!(
            renderer.placements_changed(&[placement(1, 2)]),
            "{protocol:?} moved"
        );
        assert!(
            renderer.placements_changed(&[placement(2, 2)]),
            "{protocol:?} another image"
        );
        assert!(renderer.placements_changed(&[]), "{protocol:?} gone");
    }
}

#[test]
fn kitty_and_half_blocks_never_ask_for_a_clear() {
    for protocol in [ProtocolType::Kitty, ProtocolType::Halfblocks] {
        let mut renderer = renderer(protocol, ColorDepth::TrueColor, DEFAULT_LIMIT);
        for placements in [vec![], vec![placement(1, 0)], vec![placement(1, 2)], vec![]] {
            assert!(!renderer.placements_changed(&placements), "{protocol:?}");
        }
    }
}

#[test]
fn clearing_the_screen_empties_it_and_makes_the_next_frame_draw_everything() {
    use ratatui::backend::TestBackend;
    use ratatui::widgets::Paragraph;
    use ratatui::Terminal;

    let mut terminal = Terminal::new(TestBackend::new(8, 2)).expect("terminal");
    let draw = |terminal: &mut Terminal<TestBackend>| {
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new("abc\ndef"), frame.area()))
            .expect("draws");
    };
    draw(&mut terminal);
    terminal
        .backend()
        .assert_buffer_lines(["abc     ", "def     "]);

    crate::image_view::clear_screen(&mut terminal).expect("clears");
    terminal
        .backend()
        .assert_buffer_lines(["        ", "        "]);

    // The same frame as before is drawn in full, where a difference from the
    // buffer of the last frame would have drawn nothing.
    draw(&mut terminal);
    terminal
        .backend()
        .assert_buffer_lines(["abc     ", "def     "]);
}

fn small_png() -> Vec<europa_core::Output> {
    vec![png_output(&png(100, 100))]
}

fn is_drawn(buffer: &Buffer, x: u16, y: u16) -> bool {
    buffer[(x, y)].symbol() != "x"
}

#[test]
fn a_protocol_that_fails_is_drawn_with_half_blocks() {
    let outputs = small_png();
    for protocol in [
        ProtocolType::Kitty,
        ProtocolType::Sixel,
        ProtocolType::Iterm2,
    ] {
        let mut renderer =
            super::common::renderer_with(protocol, Box::new(super::common::HalfblockOnly));
        let slot = first_slot(&mut renderer, &outputs);
        let buffer = draw_frame(
            &mut renderer,
            &outputs,
            &slot,
            Rect::new(0, 0, 12, 7),
            0,
            (12, 7),
        );
        let text = describe_protocol_frame(&buffer);
        assert!(!text.contains(['K', 'S', 'I']), "{protocol:?}\n{text}");
        for y in 0..5 {
            assert!(is_drawn(&buffer, 0, y), "{protocol:?} line {y}");
            assert!("▀▄ █".contains(buffer[(0, y)].symbol()), "{protocol:?}");
        }
    }
}

#[test]
fn when_half_blocks_fail_too_a_notice_is_drawn_and_the_outside_is_left_alone() {
    let outputs = small_png();
    let mut renderer = super::common::renderer_with(
        ProtocolType::Halfblocks,
        Box::new(super::common::FailingEncoder),
    );
    let slot = first_slot(&mut renderer, &outputs);
    let buffer = draw_frame(
        &mut renderer,
        &outputs,
        &slot,
        Rect::new(0, 0, 12, 7),
        0,
        (12, 7),
    );
    let first: String = (0..10)
        .map(|x| buffer[(x, 0)].symbol().to_owned())
        .collect();
    assert_eq!(first, "[image/png");
    assert_eq!(buffer[(0, 0)].modifier, ratatui::style::Modifier::DIM);
    let marked = marked_screen(12, 7);
    for y in 0..7u16 {
        for x in 0..12u16 {
            let inside_notice = y == 0 && x < 10;
            if !inside_notice {
                assert_eq!(buffer[(x, y)], marked[(x, y)], "({x}, {y})");
            }
        }
    }
    insta::assert_snapshot!("fallback-notice", describe(&buffer));
}

#[test]
fn the_notice_is_in_the_first_visible_line_when_the_slot_starts_above() {
    let outputs = vec![png_output(&png(100, 280))];
    let mut renderer = super::common::renderer_with(
        ProtocolType::Halfblocks,
        Box::new(super::common::FailingEncoder),
    );
    let slot = first_slot(&mut renderer, &outputs);
    let buffer = draw_frame(
        &mut renderer,
        &outputs,
        &slot,
        Rect::new(0, 2, 20, 5),
        -6,
        (20, 10),
    );
    let line: String = (0..10)
        .map(|x| buffer[(x, 2)].symbol().to_owned())
        .collect();
    assert!(line.starts_with("[image/png"), "{line:?}");
    assert_eq!(buffer[(0, 3)].symbol(), "x");
}

#[test]
fn another_image_in_the_cell_is_not_affected_by_a_failure() {
    let outputs = vec![png_output(&png(100, 100)), png_output(&png(50, 60))];
    let mut renderer = super::common::renderer_with(
        ProtocolType::Halfblocks,
        Box::new(super::common::FailsForColumns(10)),
    );
    let cell = render(&mut renderer, &outputs, 80, 200);
    let [failing, fine] = [cell.images[0], cell.images[1]];
    assert_eq!((failing.width, fine.width), (10, 5));
    let area = Rect::new(0, 0, 12, 7);
    let broken = draw_frame(&mut renderer, &outputs, &failing, area, 0, (12, 7));
    assert!(
        String::from_iter((0..10).map(|x| broken[(x, 0)].symbol().to_owned()))
            .starts_with("[image")
    );
    let shown = draw_frame(&mut renderer, &outputs, &fine, area, 0, (12, 7));
    assert!(is_drawn(&shown, 0, 0));
    assert!(
        !String::from_iter((0..5).map(|x| shown[(x, 0)].symbol().to_owned())).starts_with("[image")
    );
}

#[test]
fn a_failure_is_not_tried_again_under_the_same_conditions() {
    let outputs = small_png();
    let mut renderer =
        super::common::renderer_with(ProtocolType::Sixel, Box::new(super::common::FailingEncoder));
    let slot = first_slot(&mut renderer, &outputs);
    let area = Rect::new(0, 0, 12, 7);
    draw_frame(&mut renderer, &outputs, &slot, area, 0, (12, 7));
    let first = renderer.stats();
    assert_eq!(first.encodes, 1);
    for top in [0, 1, -1] {
        draw_frame(&mut renderer, &outputs, &slot, area, top, (12, 7));
    }
    assert_eq!(renderer.stats(), first);
}

fn placement_of(slot: &europa_core::ImageSlot) -> crate::image_view::Placement {
    crate::image_view::Placement {
        source: slot.source,
        x: 0,
        y: 0,
        width: slot.width,
        height: slot.height,
    }
}

#[test]
fn after_a_write_failure_what_was_visible_is_drawn_with_half_blocks_even_at_another_width() {
    let outputs = small_png();
    let mut renderer = renderer(ProtocolType::Sixel, ColorDepth::TrueColor, DEFAULT_LIMIT);
    let slot = first_slot(&mut renderer, &outputs);
    let area = Rect::new(0, 0, 12, 7);
    let before = draw_frame(&mut renderer, &outputs, &slot, area, 0, (12, 7));
    assert!(describe_protocol_frame(&before).contains('S'));
    renderer.placements_changed(&[placement_of(&slot)]);
    renderer.fall_back_visible();
    let after = draw_frame(&mut renderer, &outputs, &slot, area, 0, (12, 7));
    assert!(!describe_protocol_frame(&after).contains('S'));
    assert!("▀▄ █".contains(after[(0, 0)].symbol()));
    insta::assert_snapshot!("fallback-halfblock", describe(&after));

    let narrower = render(&mut renderer, &outputs, 5, 200).images[0];
    assert_ne!(narrower.width, slot.width);
    let again = draw_frame(&mut renderer, &outputs, &narrower, area, 0, (12, 7));
    assert!(!describe_protocol_frame(&again).contains('S'));
}

#[test]
fn an_image_that_was_not_passed_to_the_last_call_keeps_its_protocol() {
    let outputs = vec![png_output(&png(100, 100)), png_output(&png(50, 60))];
    let mut renderer = renderer(ProtocolType::Sixel, ColorDepth::TrueColor, DEFAULT_LIMIT);
    let cell = render(&mut renderer, &outputs, 80, 200);
    renderer.placements_changed(&[placement_of(&cell.images[0])]);
    renderer.fall_back_visible();
    let area = Rect::new(0, 0, 12, 7);
    let kept = draw_frame(&mut renderer, &outputs, &cell.images[1], area, 0, (12, 7));
    assert!(describe_protocol_frame(&kept).contains('S'));
}

#[test]
fn a_frame_that_was_not_flushed_is_replaced_by_half_blocks_after_clearing() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let outputs = small_png();
    let mut renderer = renderer(ProtocolType::Sixel, ColorDepth::TrueColor, DEFAULT_LIMIT);
    let slot = first_slot(&mut renderer, &outputs);
    let area = Rect::new(0, 0, 12, 7);
    let mut terminal = Terminal::new(TestBackend::new(12, 7)).expect("terminal");

    // A frame whose write fails: it is drawn, and never flushed or swapped.
    renderer.placements_changed(&[placement_of(&slot)]);
    renderer.draw(terminal.get_frame().buffer_mut(), area, &outputs, &slot, 0);

    renderer.fall_back_visible();
    crate::image_view::clear_screen(&mut terminal).expect("clears");
    terminal
        .draw(|frame| renderer.draw(frame.buffer_mut(), area, &outputs, &slot, 0))
        .expect("draws");
    let screen = terminal.backend().buffer();
    for y in 0..5 {
        assert!(
            "▀▄ █".contains(screen[(0, y)].symbol()),
            "line {y}: {:?}",
            screen[(0, y)]
        );
        assert_ne!(screen[(0, y)].fg, ratatui::style::Color::Reset, "line {y}");
    }
}
