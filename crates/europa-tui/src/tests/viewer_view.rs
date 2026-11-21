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

fn ten_cells_notebook() -> europa_core::Notebook {
    let cells: Vec<String> = (0..10)
        .map(|i| {
            common::code_cell(
                &(0..10).map(|n| format!("x = {n}\n")).collect::<String>(),
                Some(i as i64),
                &[common::stream(
                    "stdout",
                    &(0..20).map(|n| format!("line {n}\n")).collect::<String>(),
                )],
            )
        })
        .collect();
    common::notebook(&cells)
}

fn apply(
    viewer: &mut crate::viewer::Viewer,
    recorder: &mut common::RecordingRenderer<'_>,
    key: crate::viewer::Key,
) {
    let _ = crate::viewer::update(viewer, crate::viewer::Input::Key(key), recorder);
    let _ = recorder.take_calls();
}

#[test]
fn next_cell_snapshot() {
    let notebook = ten_cells_notebook();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..3 {
        apply(&mut viewer, &mut recorder, crate::viewer::Key::NextCell);
    }
    for _ in 0..4 {
        apply(&mut viewer, &mut recorder, crate::viewer::Key::Down);
    }
    apply(&mut viewer, &mut recorder, crate::viewer::Key::NextCell);
    insta::assert_snapshot!("next_cell", common::frame(&viewer, &notebook));
}

#[test]
fn prev_cell_middle_snapshot() {
    let notebook = ten_cells_notebook();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..3 {
        apply(&mut viewer, &mut recorder, crate::viewer::Key::NextCell);
    }
    for _ in 0..4 {
        apply(&mut viewer, &mut recorder, crate::viewer::Key::Down);
    }
    apply(&mut viewer, &mut recorder, crate::viewer::Key::PrevCell);
    insta::assert_snapshot!("prev_cell_middle", common::frame(&viewer, &notebook));
}

#[test]
fn prev_cell_first_snapshot() {
    let notebook = ten_cells_notebook();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    apply(&mut viewer, &mut recorder, crate::viewer::Key::PrevCell);
    insta::assert_snapshot!("prev_cell_first", common::frame(&viewer, &notebook));
}

#[test]
fn next_cell_bottom_snapshot() {
    let notebook = ten_cells_notebook();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..20 {
        apply(&mut viewer, &mut recorder, crate::viewer::Key::NextCell);
    }
    insta::assert_snapshot!("next_cell_bottom", common::frame(&viewer, &notebook));
}

#[test]
fn gg_snapshot() {
    let notebook = ten_cells_notebook();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..5 {
        apply(&mut viewer, &mut recorder, crate::viewer::Key::Down);
    }
    apply(&mut viewer, &mut recorder, crate::viewer::Key::GPrefix);
    apply(&mut viewer, &mut recorder, crate::viewer::Key::GPrefix);
    insta::assert_snapshot!("gg", common::frame(&viewer, &notebook));
}

#[test]
fn big_g_snapshot() {
    let notebook = ten_cells_notebook();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    apply(&mut viewer, &mut recorder, crate::viewer::Key::Bottom);
    insta::assert_snapshot!("big_g", common::frame(&viewer, &notebook));
}

#[test]
fn g_then_other_snapshot() {
    let notebook = ten_cells_notebook();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    apply(&mut viewer, &mut recorder, crate::viewer::Key::GPrefix);
    apply(&mut viewer, &mut recorder, crate::viewer::Key::Down);
    insta::assert_snapshot!("g_then_other", common::frame(&viewer, &notebook));
}

#[test]
fn modified_key_snapshot() {
    let notebook = ten_cells_notebook();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    apply(&mut viewer, &mut recorder, crate::viewer::Key::GPrefix);
    apply(&mut viewer, &mut recorder, crate::viewer::Key::Other);
    insta::assert_snapshot!("modified_key", common::frame(&viewer, &notebook));
}

fn step(
    notebook: &europa_core::Notebook,
    viewer: &mut crate::viewer::Viewer,
    recorder: &mut common::RecordingRenderer<'_>,
    code: ratatui::crossterm::event::KeyCode,
) {
    let _ = notebook;
    let input = common::key(code);
    let _ = crate::viewer::update(viewer, input, recorder);
    let _ = recorder.take_calls();
}

#[test]
fn move_j_snapshot() {
    let notebook = common::large_notebook(10);
    let (mut viewer, mut recorder) = common::open(&notebook, 40, 12);
    let _ = recorder.take_calls();
    step(
        &notebook,
        &mut viewer,
        &mut recorder,
        ratatui::crossterm::event::KeyCode::Char('j'),
    );
    insta::assert_snapshot!("move_j", common::frame(&viewer, &notebook));
}

#[test]
fn move_k_top_snapshot() {
    let notebook = common::large_notebook(10);
    let (mut viewer, mut recorder) = common::open(&notebook, 40, 12);
    let _ = recorder.take_calls();
    step(
        &notebook,
        &mut viewer,
        &mut recorder,
        ratatui::crossterm::event::KeyCode::Char('k'),
    );
    insta::assert_snapshot!("move_k_top", common::frame(&viewer, &notebook));
}

#[test]
fn move_j_bottom_snapshot() {
    let cells: Vec<String> = (0..3)
        .map(|_| {
            common::code_cell(
                &(0..10).map(|n| format!("x = {n}\n")).collect::<String>(),
                Some(0),
                &[common::stream(
                    "stdout",
                    &(0..20).map(|n| format!("line {n}\n")).collect::<String>(),
                )],
            )
        })
        .collect();
    let notebook = common::notebook(&cells);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..100 {
        step(
            &notebook,
            &mut viewer,
            &mut recorder,
            ratatui::crossterm::event::KeyCode::Char('j'),
        );
    }
    insta::assert_snapshot!("move_j_bottom", common::frame(&viewer, &notebook));
}

#[test]
fn move_arrows_snapshot() {
    let notebook = common::large_notebook(10);
    let (mut viewer, mut recorder) = common::open(&notebook, 40, 12);
    let _ = recorder.take_calls();
    step(
        &notebook,
        &mut viewer,
        &mut recorder,
        ratatui::crossterm::event::KeyCode::Down,
    );
    step(
        &notebook,
        &mut viewer,
        &mut recorder,
        ratatui::crossterm::event::KeyCode::Up,
    );
    insta::assert_snapshot!("move_arrows", common::frame(&viewer, &notebook));
}

#[test]
fn tall_cell_snapshot() {
    let tall_source: String = (0..49).map(|n| format!("line {n}\n")).collect();
    let notebook = common::notebook(&[common::code_cell(&tall_source, Some(0), &[])]);
    let (mut viewer, mut recorder) = common::open(&notebook, 40, 12);
    let _ = recorder.take_calls();
    for _ in 0..5 {
        step(
            &notebook,
            &mut viewer,
            &mut recorder,
            ratatui::crossterm::event::KeyCode::Char('j'),
        );
    }
    insta::assert_snapshot!("tall_cell", common::frame(&viewer, &notebook));
}

#[test]
fn resize_width_snapshot() {
    let notebook = common::large_notebook(10);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    let _ = crate::viewer::update(
        &mut viewer,
        crate::viewer::Input::Resize {
            columns: 40,
            rows: 40,
            cell_pixels: europa_core::CellPixels::DEFAULT,
        },
        &mut recorder,
    );
    insta::assert_snapshot!("resize_width", common::frame(&viewer, &notebook));
}

#[test]
fn resize_height_snapshot() {
    let notebook = common::large_notebook(10);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    let _ = crate::viewer::update(
        &mut viewer,
        crate::viewer::Input::Resize {
            columns: 80,
            rows: 20,
            cell_pixels: europa_core::CellPixels::DEFAULT,
        },
        &mut recorder,
    );
    insta::assert_snapshot!("resize_height", common::frame(&viewer, &notebook));
}

#[test]
fn resize_mid_cell_snapshot() {
    let notebook = common::large_notebook(10);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..10 {
        step(
            &notebook,
            &mut viewer,
            &mut recorder,
            ratatui::crossterm::event::KeyCode::Char('j'),
        );
    }
    let _ = crate::viewer::update(
        &mut viewer,
        crate::viewer::Input::Resize {
            columns: 40,
            rows: 40,
            cell_pixels: europa_core::CellPixels::DEFAULT,
        },
        &mut recorder,
    );
    insta::assert_snapshot!("resize_mid_cell", common::frame(&viewer, &notebook));
}

#[test]
fn resize_one_row_snapshot() {
    let notebook = common::large_notebook(10);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    let _ = crate::viewer::update(
        &mut viewer,
        crate::viewer::Input::Resize {
            columns: 80,
            rows: 1,
            cell_pixels: europa_core::CellPixels::DEFAULT,
        },
        &mut recorder,
    );
    insta::assert_snapshot!("resize_one_row", common::frame(&viewer, &notebook));
}

#[test]
fn status_bottom_snapshot() {
    let cells: Vec<String> = (0..3)
        .map(|_| {
            common::code_cell(
                &(0..10).map(|n| format!("x = {n}\n")).collect::<String>(),
                Some(0),
                &[common::stream(
                    "stdout",
                    &(0..20).map(|n| format!("line {n}\n")).collect::<String>(),
                )],
            )
        })
        .collect();
    let notebook = common::notebook(&cells);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..100 {
        step(
            &notebook,
            &mut viewer,
            &mut recorder,
            ratatui::crossterm::event::KeyCode::Char('j'),
        );
    }
    insta::assert_snapshot!("status_bottom", common::frame(&viewer, &notebook));
}

#[test]
fn image_edge_snapshots() {
    let bytes = common::png(64, 120);
    let data = base64_simd::STANDARD.encode_to_string(&bytes);
    let output = format!("{{\"output_type\":\"display_data\",\"data\":{{\"image/png\":\"{data}\"}},\"metadata\":{{}}}}");
    let image_cell = common::code_cell("a\nb\n", Some(1), &[output]);
    let rest: Vec<String> = (0..2)
        .map(|_| {
            common::code_cell(
                &(0..10).map(|n| format!("x = {n}\n")).collect::<String>(),
                Some(1),
                &[common::stream(
                    "stdout",
                    &(0..20).map(|n| format!("line {n}\n")).collect::<String>(),
                )],
            )
        })
        .collect();
    let mut cells = vec![image_cell];
    cells.extend(rest);
    let notebook = common::notebook(&cells);
    let options = europa_core::RenderOptions::new(std::num::NonZeroU16::new(40).expect("non-zero"));
    let pixels = europa_core::CellPixels {
        width: std::num::NonZeroU16::new(10).expect("non-zero"),
        height: std::num::NonZeroU16::new(20).expect("non-zero"),
    };
    let options = europa_core::RenderOptions {
        cell_pixels: pixels,
        ..options
    };
    let renderer = crate::image_view::ImageRenderer::with_parts(
        ratatui_image::picker::ProtocolType::Halfblocks,
        pixels,
        crate::image_protocol::ColorDepth::TrueColor,
        crate::image_cache::DEFAULT_LIMIT,
        Box::new(crate::image_view::RatatuiEncoder),
    );
    let (mut viewer, mut recorder) = common::open_with_images(&notebook, 40, 12, options, renderer);
    let _ = recorder.take_calls();
    if let Some(images) = recorder.images.as_mut() {
        let _ = common::frame_in(&viewer, &notebook, images);
    }
    step(
        &notebook,
        &mut viewer,
        &mut recorder,
        ratatui::crossterm::event::KeyCode::Char('j'),
    );
    if let Some(images) = recorder.images.as_mut() {
        let text = common::frame_in(&viewer, &notebook, images);
        insta::assert_snapshot!("image_edge_top", text);
    }
    for _ in 0..5 {
        step(
            &notebook,
            &mut viewer,
            &mut recorder,
            ratatui::crossterm::event::KeyCode::Char('j'),
        );
    }
    if let Some(images) = recorder.images.as_mut() {
        let text = common::frame_in(&viewer, &notebook, images);
        insta::assert_snapshot!("image_edge_bottom", text);
    }
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
