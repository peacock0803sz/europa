use std::collections::HashMap;
use std::num::NonZeroU16;

use europa_core::{CellPixels, ImageProtocolChoice};
use ratatui_image::picker::{Capability, ProtocolType};

use crate::image_protocol::{
    cell_pixels_from, color_depth, plan_selection, probe_from, ColorDepth, Probe, Selection,
};

fn cell(width: u16, height: u16) -> CellPixels {
    CellPixels {
        width: NonZeroU16::new(width).expect("non-zero"),
        height: NonZeroU16::new(height).expect("non-zero"),
    }
}

fn probe(protocol: ProtocolType, cell_pixels: Option<CellPixels>) -> Option<Probe> {
    Some(Probe {
        protocol,
        cell_pixels,
    })
}

#[test]
fn auto_asks_the_terminal_without_naming_a_protocol() {
    assert_eq!(
        plan_selection(ImageProtocolChoice::Auto, true),
        Selection::Query { force: None }
    );
}

#[test]
fn half_blocks_never_ask_the_terminal() {
    assert_eq!(
        plan_selection(ImageProtocolChoice::Halfblock, true),
        Selection::Halfblock
    );
}

#[test]
fn a_session_that_is_not_interactive_never_asks_the_terminal() {
    for choice in [
        ImageProtocolChoice::Auto,
        ImageProtocolChoice::Kitty,
        ImageProtocolChoice::Sixel,
        ImageProtocolChoice::Iterm2,
        ImageProtocolChoice::Halfblock,
    ] {
        assert_eq!(plan_selection(choice, false), Selection::Halfblock);
    }
}

#[test]
fn half_blocks_use_the_window_and_ignore_the_query() {
    let (protocol, size) = Selection::Halfblock.resolve(None, Some(cell(8, 16)));
    assert_eq!((protocol, size), (ProtocolType::Halfblocks, cell(8, 16)));
    let (protocol, size) = Selection::Halfblock.resolve(None, None);
    assert_eq!(
        (protocol, size),
        (ProtocolType::Halfblocks, CellPixels::DEFAULT)
    );
    let (protocol, size) = Selection::Halfblock.resolve(
        probe(ProtocolType::Kitty, Some(cell(9, 18))),
        Some(cell(8, 16)),
    );
    assert_eq!((protocol, size), (ProtocolType::Halfblocks, cell(8, 16)));
}

#[test]
fn a_detected_protocol_is_used_with_the_size_of_the_query_or_the_window() {
    let query = Selection::Query { force: None };
    assert_eq!(
        query.resolve(probe(ProtocolType::Kitty, Some(cell(9, 18))), None),
        (ProtocolType::Kitty, cell(9, 18))
    );
    assert_eq!(
        query.resolve(probe(ProtocolType::Kitty, None), Some(cell(8, 16))),
        (ProtocolType::Kitty, cell(8, 16))
    );
    assert_eq!(
        query.resolve(
            probe(ProtocolType::Kitty, Some(cell(9, 18))),
            Some(cell(8, 16))
        ),
        (ProtocolType::Kitty, cell(9, 18))
    );
}

#[test]
fn a_protocol_without_a_known_cell_size_falls_back_to_half_blocks() {
    let query = Selection::Query { force: None };
    assert_eq!(
        query.resolve(probe(ProtocolType::Iterm2, None), None),
        (ProtocolType::Halfblocks, CellPixels::DEFAULT)
    );
}

#[test]
fn a_terminal_that_found_nothing_gets_half_blocks() {
    let query = Selection::Query { force: None };
    assert_eq!(
        query.resolve(probe(ProtocolType::Halfblocks, None), None),
        (ProtocolType::Halfblocks, CellPixels::DEFAULT)
    );
}

#[test]
fn a_failed_query_still_uses_the_window() {
    let query = Selection::Query { force: None };
    assert_eq!(
        query.resolve(None, Some(cell(8, 16))),
        (ProtocolType::Halfblocks, cell(8, 16))
    );
}

#[test]
fn the_cell_size_of_the_query_needs_both_sides_to_be_positive() {
    let reported = |size| vec![Capability::CellSize(size)];
    let size = probe_from(ProtocolType::Kitty, &reported(Some((9, 18)))).cell_pixels;
    assert_eq!(size, Some(cell(9, 18)));
    for size in [Some((0, 18)), Some((9, 0)), None] {
        assert_eq!(
            probe_from(ProtocolType::Kitty, &reported(size)).cell_pixels,
            None
        );
    }
    assert_eq!(probe_from(ProtocolType::Kitty, &[]).cell_pixels, None);
}

#[test]
fn the_protocol_of_a_probe_is_the_one_it_was_given() {
    for protocol in [
        ProtocolType::Halfblocks,
        ProtocolType::Sixel,
        ProtocolType::Kitty,
        ProtocolType::Iterm2,
    ] {
        assert_eq!(
            probe_from(protocol, &[Capability::Kitty]).protocol,
            protocol
        );
    }
}

fn depth(vars: &[(&str, &str)]) -> ColorDepth {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    color_depth(|name| vars.get(name).cloned())
}

#[test]
fn true_color_is_taken_from_the_environment() {
    assert_eq!(depth(&[("COLORTERM", "truecolor")]), ColorDepth::TrueColor);
    assert_eq!(depth(&[("COLORTERM", "24bit")]), ColorDepth::TrueColor);
    assert_eq!(depth(&[("TERM", "xterm-direct")]), ColorDepth::TrueColor);
}

#[test]
fn anything_else_is_256_colors() {
    assert_eq!(depth(&[]), ColorDepth::Indexed256);
    assert_eq!(depth(&[("COLORTERM", "yes")]), ColorDepth::Indexed256);
    assert_eq!(depth(&[("TERM", "xterm-256color")]), ColorDepth::Indexed256);
    assert_eq!(depth(&[("TERM", "direct-ish")]), ColorDepth::Indexed256);
}

#[test]
fn a_cell_is_the_window_divided_by_its_columns_and_lines() {
    assert_eq!(cell_pixels_from(80, 24, 800, 480), Some(cell(10, 20)));
}

#[test]
fn a_window_that_does_not_tell_gives_no_cell_size() {
    for (columns, rows, width, height) in [
        (0, 24, 800, 480),
        (80, 0, 800, 480),
        (80, 24, 0, 480),
        (80, 24, 800, 0),
        (80, 24, 40, 480),
        (80, 24, 800, 12),
    ] {
        assert_eq!(cell_pixels_from(columns, rows, width, height), None);
    }
}
