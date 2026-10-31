use std::num::NonZeroU16;

use europa_core::{
    Background, Color, ImageSlot, RenderOptions, RenderedCell, Style, StyledLine, StyledSpan,
};
use serde::Serialize;

fn width(columns: u16) -> NonZeroU16 {
    NonZeroU16::new(columns).expect("non-zero")
}

fn options(json: &str) -> Result<RenderOptions, serde_json::Error> {
    serde_json::from_str(json)
}

fn json(value: &impl Serialize) -> String {
    serde_json::to_string(value).expect("serializes")
}

#[test]
fn new_options_use_the_default_limit_and_a_dark_background() {
    let options = RenderOptions::new(width(80));
    assert_eq!(options.width.get(), 80);
    assert_eq!(options.max_output_lines.get(), 200);
    assert_eq!(options.background, Background::Dark);
    assert_eq!(RenderOptions::DEFAULT_MAX_OUTPUT_LINES.get(), 200);
}

#[test]
fn options_read_from_serde_default_what_they_leave_out() {
    let read = options(r#"{"width":20}"#).expect("reads");
    assert_eq!(read.max_output_lines.get(), 200);
    assert_eq!(read.background, Background::Dark);
    assert_eq!(read, RenderOptions::new(width(20)));
}

#[test]
fn options_read_from_serde_keep_what_they_give() {
    let read =
        options(r#"{"width":20,"max_output_lines":10,"background":"light"}"#).expect("reads");
    assert_eq!(read.width.get(), 20);
    assert_eq!(read.max_output_lines.get(), 10);
    assert_eq!(read.background, Background::Light);
}

#[test]
fn a_zero_width_or_limit_does_not_read() {
    assert!(options(r#"{"width":0}"#).is_err());
    assert!(options(r#"{"width":20,"max_output_lines":0}"#).is_err());
}

#[test]
fn colors_serialize_as_an_indexed_or_rgb_object() {
    assert_eq!(json(&Color::Indexed(1)), r#"{"indexed":1}"#);
    assert_eq!(json(&Color::Rgb(1, 2, 3)), r#"{"rgb":[1,2,3]}"#);
}

#[test]
fn styles_leave_out_what_is_not_set() {
    assert_eq!(json(&Style::default()), "{}");
    let style = Style {
        fg: Some(Color::Indexed(1)),
        bold: true,
        ..Style::default()
    };
    assert_eq!(json(&style), r#"{"fg":{"indexed":1},"bold":true}"#);
}

#[test]
fn lines_always_list_their_spans_and_leave_out_a_missing_fill() {
    assert_eq!(json(&StyledLine::default()), r#"{"spans":[]}"#);
    let line = StyledLine {
        spans: Vec::new(),
        fill: Some(Color::Indexed(52)),
    };
    assert_eq!(json(&line), r#"{"spans":[],"fill":{"indexed":52}}"#);
}

#[test]
fn background_serializes_in_snake_case() {
    assert_eq!(json(&Background::Dark), r#""dark""#);
    assert_eq!(json(&Background::Light), r#""light""#);
}

#[test]
fn an_empty_cell_has_no_lines_and_no_images() {
    let cell = RenderedCell::default();
    assert!(cell.lines.is_empty());
    assert!(cell.images.is_empty());
}

#[test]
fn a_span_without_a_style_reads_as_plain_text() {
    let span: StyledSpan = serde_json::from_str(r#"{"text":"a"}"#).expect("reads");
    assert_eq!(span.text, "a");
    assert_eq!(span.style, Style::default());
}

#[test]
fn a_rendered_cell_reads_back_as_written() {
    let cell = RenderedCell {
        lines: vec![StyledLine {
            spans: vec![
                StyledSpan {
                    text: "a".to_owned(),
                    style: Style::default(),
                },
                StyledSpan {
                    text: "b".to_owned(),
                    style: Style {
                        bg: Some(Color::Rgb(1, 2, 3)),
                        underline: true,
                        ..Style::default()
                    },
                },
            ],
            fill: Some(Color::Indexed(52)),
        }],
        images: vec![ImageSlot {
            line: 0,
            width: 4,
            height: 2,
        }],
    };
    let read: RenderedCell = serde_json::from_str(&json(&cell)).expect("reads");
    assert_eq!(read, cell);
}
