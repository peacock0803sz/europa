//! Frame snapshots for the first screen.

use std::num::{NonZeroU16, NonZeroUsize};

use europa_core::{Background, RenderOptions};

use crate::tests::common;

fn opts_40_12() -> (u16, u16) {
    (40, 12)
}

#[test]
fn open_kinds_snapshot() {
    let (columns, rows) = opts_40_12();
    let notebook = common::notebook(&[
        common::code_cell("x = 1\n", Some(0), &[common::stream("stdout", "hi\n")]),
        common::markdown_cell("# Title\n"),
        common::raw_cell("raw\n"),
        common::unknown_cell("widget"),
        common::code_cell("", None, &[]),
    ]);
    let (viewer, _) = common::open(&notebook, columns, rows);
    insta::assert_snapshot!("open_kinds", common::frame(&viewer, &notebook));
}

#[test]
fn open_empty_snapshot() {
    let notebook = common::notebook(&[]);
    let (viewer, _) = common::open(&notebook, 40, 12);
    insta::assert_snapshot!("open_empty", common::frame(&viewer, &notebook));
}

#[test]
fn open_small_snapshot() {
    let notebook = common::notebook(&[common::code_cell("x\n", Some(1), &[])]);
    let (viewer, _) = common::open(&notebook, 40, 12);
    insta::assert_snapshot!("open_small", common::frame(&viewer, &notebook));
}

#[test]
fn truncated_output_snapshot() {
    let text: String = (0..12).map(|n| format!("line {n}\n")).collect();
    let notebook = common::notebook(&[common::code_cell(
        "x\n",
        Some(1),
        &[common::stream("stdout", &text)],
    )]);
    let options = RenderOptions {
        max_output_lines: NonZeroUsize::new(5).expect("non-zero"),
        ..RenderOptions::new(NonZeroU16::new(40).expect("non-zero"))
    };
    let (viewer, _) = common::open_with(&notebook, 40, 12, options);
    insta::assert_snapshot!("truncated_output", common::frame(&viewer, &notebook));
}

#[test]
fn background_dark_snapshot() {
    let cells = [common::code_cell(
        "x\n",
        Some(1),
        &[
            common::stream("stderr", "warn\n"),
            common::error_output("E", "bad"),
        ],
    )];
    let notebook = common::notebook(&cells);
    let (viewer, _) = common::open(&notebook, 40, 12);
    insta::assert_snapshot!("background_dark", common::frame(&viewer, &notebook));
}

#[test]
fn background_light_snapshot() {
    let cells = [common::code_cell(
        "x\n",
        Some(1),
        &[
            common::stream("stderr", "warn\n"),
            common::error_output("E", "bad"),
        ],
    )];
    let notebook = common::notebook(&cells);
    let options = RenderOptions {
        background: Background::Light,
        ..RenderOptions::new(NonZeroU16::new(40).expect("non-zero"))
    };
    let (viewer, _) = common::open_with(&notebook, 40, 12, options);
    insta::assert_snapshot!("background_light", common::frame(&viewer, &notebook));
}

#[test]
fn status_many_snapshot() {
    let many = common::large_notebook(100);
    let (viewer, _) = common::open(&many, 40, 12);
    insta::assert_snapshot!("status_many", common::frame(&viewer, &many));
}

#[test]
fn status_one_snapshot() {
    let one = common::notebook(&[common::code_cell("x\n", Some(1), &[])]);
    let (viewer, _) = common::open(&one, 40, 12);
    insta::assert_snapshot!("status_one", common::frame(&viewer, &one));
}

#[test]
fn status_none_snapshot() {
    let many = common::large_notebook(100);
    let (viewer, _) = common::open(&many, 40, 1);
    insta::assert_snapshot!("status_none", common::frame(&viewer, &many));
}

#[test]
fn status_narrow_snapshot() {
    let many = common::large_notebook(100);
    let (viewer, _) = common::open(&many, 12, 12);
    insta::assert_snapshot!("status_narrow", common::frame(&viewer, &many));
}

#[test]
fn narrow_widths_do_not_fail() {
    for width in [1, 2] {
        let notebook = common::notebook(&[common::code_cell("x\n", Some(1), &[])]);
        let (viewer, _) = common::open(&notebook, width, 12);
        let _ = common::frame(&viewer, &notebook);
    }
}

#[test]
fn view_does_not_render() {
    let notebook = common::large_notebook(10);
    let (viewer, mut recorder) = common::open(&notebook, 40, 12);
    let _ = recorder.take_calls();
    let _ = common::frame(&viewer, &notebook);
    assert!(recorder.take_calls().is_empty());
}

#[test]
fn file_names_lose_controls() {
    use std::path::Path;
    assert_eq!(
        crate::viewer_view::status_name(Path::new("dir/a\u{1b}b.ipynb")),
        "a␛b.ipynb"
    );
    assert_eq!(
        crate::viewer_view::status_name(Path::new("plain.ipynb")),
        "plain.ipynb"
    );
}
