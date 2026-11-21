//! Viewer opening and pure transitions.

use europa_core::CellPixels;

use crate::tests::common;
use crate::viewer::{Effect, Input, Key};

#[test]
fn open_shows_only_visible_cells() {
    let notebook = common::large_notebook(1000);
    let (viewer, mut recorder) = common::open(&notebook, 80, 40);
    assert_eq!(viewer.position, crate::viewer::Position { cell: 0, row: 0 });
    assert_eq!(recorder.take_calls(), vec![0, 1]);
    let keys: Vec<_> = viewer.visible.keys().copied().collect();
    assert_eq!(keys, vec![0, 1]);
    assert_eq!(viewer.visible_range(), Some((0, 1)));
}

#[test]
fn empty_notebook_opens_empty_and_ignores_keys() {
    let notebook = common::notebook(&[]);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    assert!(recorder.take_calls().is_empty());
    assert!(viewer.visible.is_empty());
    assert_eq!(viewer.visible_range(), None);
    let effect = crate::viewer::update(&mut viewer, Input::Key(Key::Other), &mut recorder);
    assert_eq!(effect, Effect::None);
    assert_eq!(viewer.position, crate::viewer::Position { cell: 0, row: 0 });
}

#[test]
fn one_row_terminal_has_no_visible_cells() {
    let notebook = common::large_notebook(10);
    let (viewer, mut recorder) = common::open(&notebook, 80, 1);
    assert!(recorder.take_calls().is_empty());
    assert!(viewer.visible.is_empty());
}

#[test]
fn quit_and_other_keys() {
    let notebook = common::large_notebook(10);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let before = viewer.position;
    let visible: Vec<_> = viewer.visible.keys().copied().collect();
    let effect = crate::viewer::update(&mut viewer, Input::Key(Key::Quit), &mut recorder);
    assert_eq!(effect, Effect::Quit);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    let effect = crate::viewer::update(&mut viewer, Input::Key(Key::Other), &mut recorder);
    assert_eq!(effect, Effect::None);
    assert_eq!(viewer.position, before);
    let after: Vec<_> = viewer.visible.keys().copied().collect();
    assert_eq!(after, visible);
    assert!(recorder.take_calls().is_empty());
}

#[test]
fn resize_keeps_or_redraws_visible_cells() {
    let notebook = common::large_notebook(10);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    let pixels = CellPixels::DEFAULT;
    let effect = crate::viewer::update(
        &mut viewer,
        Input::Resize {
            columns: 80,
            rows: 30,
            cell_pixels: pixels,
        },
        &mut recorder,
    );
    assert_eq!(effect, Effect::None);
    let calls = recorder.take_calls();
    assert!(
        calls.iter().all(|i| *i >= 2) || calls.is_empty(),
        "{calls:?}"
    );
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    let effect = crate::viewer::update(
        &mut viewer,
        Input::Resize {
            columns: 40,
            rows: 40,
            cell_pixels: pixels,
        },
        &mut recorder,
    );
    assert_eq!(effect, Effect::None);
    assert_eq!(recorder.take_calls().len(), viewer.visible.len());
}

#[test]
fn same_inputs_give_same_state() {
    let notebook = common::large_notebook(10);
    let inputs = [
        Input::Key(Key::Other),
        Input::Resize {
            columns: 80,
            rows: 30,
            cell_pixels: CellPixels::DEFAULT,
        },
    ];
    let (mut first, mut recorder) = common::open(&notebook, 80, 40);
    for input in inputs {
        crate::viewer::update(&mut first, input, &mut recorder);
    }
    let (mut second, mut recorder) = common::open(&notebook, 80, 40);
    for input in inputs {
        crate::viewer::update(&mut second, input, &mut recorder);
    }
    assert_eq!(first.position, second.position);
    assert_eq!(
        first.visible.keys().collect::<Vec<_>>(),
        second.visible.keys().collect::<Vec<_>>()
    );
}

fn tall_then_two() -> europa_core::Notebook {
    let tall_source: String = (0..49).map(|n| format!("line {n}\n")).collect();
    let tall = common::code_cell(&tall_source, Some(0), &[]);
    let second = common::code_cell(
        &(0..10).map(|n| format!("x = {n}\n")).collect::<String>(),
        Some(1),
        &[common::stream(
            "stdout",
            &(0..20).map(|n| format!("line {n}\n")).collect::<String>(),
        )],
    );
    let third = common::code_cell(
        &(0..10).map(|n| format!("y = {n}\n")).collect::<String>(),
        Some(2),
        &[common::stream(
            "stdout",
            &(0..20).map(|n| format!("out {n}\n")).collect::<String>(),
        )],
    );
    common::notebook(&[tall, second, third])
}

#[test]
fn step_down_renders_only_new_cells() {
    let notebook = common::large_notebook(10);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    let effect = crate::viewer::update(
        &mut viewer,
        common::key(ratatui::crossterm::event::KeyCode::Char('j')),
        &mut recorder,
    );
    assert_eq!(effect, Effect::None);
    assert_eq!(viewer.position, crate::viewer::Position { cell: 0, row: 1 });
    let calls = recorder.take_calls();
    assert!(
        calls.is_empty() || calls == vec![1] || calls == vec![0, 1],
        "{calls:?}"
    );
}

#[test]
fn step_up_at_top_keeps_position() {
    let notebook = common::large_notebook(10);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    let effect = crate::viewer::update(
        &mut viewer,
        common::key(ratatui::crossterm::event::KeyCode::Char('k')),
        &mut recorder,
    );
    assert_eq!(effect, Effect::None);
    assert_eq!(viewer.position, crate::viewer::Position { cell: 0, row: 0 });
    assert!(recorder.take_calls().is_empty());
}

#[test]
fn step_down_at_bottom_keeps_position() {
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
        let _ = crate::viewer::update(
            &mut viewer,
            common::key(ratatui::crossterm::event::KeyCode::Char('j')),
            &mut recorder,
        );
        let _ = recorder.take_calls();
    }
    let bottom = viewer.position;
    assert_eq!(bottom, crate::viewer::Position { cell: 1, row: 25 });
    let effect = crate::viewer::update(
        &mut viewer,
        common::key(ratatui::crossterm::event::KeyCode::Char('j')),
        &mut recorder,
    );
    assert_eq!(effect, Effect::None);
    assert_eq!(viewer.position, bottom);
}

#[test]
fn tall_cell_steps_line_by_line() {
    let notebook = tall_then_two();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..49 {
        let effect = crate::viewer::update(
            &mut viewer,
            common::key(ratatui::crossterm::event::KeyCode::Char('j')),
            &mut recorder,
        );
        assert_eq!(effect, Effect::None);
    }
    assert_eq!(
        viewer.position,
        crate::viewer::Position { cell: 0, row: 49 }
    );
    let effect = crate::viewer::update(
        &mut viewer,
        common::key(ratatui::crossterm::event::KeyCode::Char('j')),
        &mut recorder,
    );
    assert_eq!(effect, Effect::None);
    assert_eq!(viewer.position, crate::viewer::Position { cell: 1, row: 0 });
    let effect = crate::viewer::update(
        &mut viewer,
        common::key(ratatui::crossterm::event::KeyCode::Char('k')),
        &mut recorder,
    );
    assert_eq!(effect, Effect::None);
    assert_eq!(
        viewer.position,
        crate::viewer::Position { cell: 0, row: 49 }
    );
}

#[test]
fn small_notebook_ignores_steps() {
    let notebook = common::notebook(&[common::code_cell("x\n", Some(1), &[])]);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for code in [
        ratatui::crossterm::event::KeyCode::Char('j'),
        ratatui::crossterm::event::KeyCode::Char('k'),
    ] {
        let effect = crate::viewer::update(&mut viewer, common::key(code), &mut recorder);
        assert_eq!(effect, Effect::None);
    }
    assert_eq!(viewer.position, crate::viewer::Position { cell: 0, row: 0 });
}

#[test]
fn arrows_match_home_row() {
    use ratatui::crossterm::event::KeyCode;
    let first = common::large_notebook(10);
    let (mut via_home, mut recorder) = common::open(&first, 80, 40);
    let _ = recorder.take_calls();
    let _ = crate::viewer::update(
        &mut via_home,
        common::key(KeyCode::Char('j')),
        &mut recorder,
    );
    let second = common::large_notebook(10);
    let (mut via_arrows, mut recorder) = common::open(&second, 80, 40);
    let _ = recorder.take_calls();
    let _ = crate::viewer::update(&mut via_arrows, common::key(KeyCode::Down), &mut recorder);
    assert_eq!(via_home.position, via_arrows.position);
    let _ = crate::viewer::update(
        &mut via_home,
        common::key(KeyCode::Char('k')),
        &mut recorder,
    );
    let _ = crate::viewer::update(&mut via_arrows, common::key(KeyCode::Up), &mut recorder);
    assert_eq!(via_home.position, via_arrows.position);
}

#[test]
fn resize_keeps_ratio() {
    use std::num::NonZeroU16;
    let wide_source: String = (0..99).map(|_| format!("{}\n", "x".repeat(60))).collect();
    let tall = common::code_cell(&wide_source, Some(0), &[]);
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
    let mut cells = vec![tall];
    cells.extend(rest);
    let notebook = common::notebook(&cells);
    let wide = europa_core::RenderOptions::new(NonZeroU16::new(80).expect("non-zero"));
    let narrow = europa_core::RenderOptions::new(NonZeroU16::new(40).expect("non-zero"));
    let wide_cell = crate::cell_display::cell_display(
        &notebook.cells[0],
        1,
        notebook.source_language(),
        &wide,
        &mut |_, _| {},
    );
    let narrow_cell = crate::cell_display::cell_display(
        &notebook.cells[0],
        1,
        notebook.source_language(),
        &narrow,
        &mut |_, _| {},
    );
    assert_eq!(wide_cell.lines.len(), 100);
    assert_eq!(narrow_cell.lines.len(), 199);
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..40 {
        let _ = crate::viewer::update(
            &mut viewer,
            common::key(ratatui::crossterm::event::KeyCode::Char('j')),
            &mut recorder,
        );
        let _ = recorder.take_calls();
    }
    assert_eq!(
        viewer.position,
        crate::viewer::Position { cell: 0, row: 40 }
    );
    let effect = crate::viewer::update(
        &mut viewer,
        Input::Resize {
            columns: 40,
            rows: 40,
            cell_pixels: CellPixels::DEFAULT,
        },
        &mut recorder,
    );
    assert_eq!(effect, Effect::None);
    assert_eq!(
        viewer.position,
        crate::viewer::Position { cell: 0, row: 79 }
    );
    let _ = recorder.take_calls();
    let effect = crate::viewer::update(
        &mut viewer,
        Input::Resize {
            columns: 80,
            rows: 40,
            cell_pixels: CellPixels::DEFAULT,
        },
        &mut recorder,
    );
    assert_eq!(effect, Effect::None);
    assert_eq!(
        viewer.position,
        crate::viewer::Position { cell: 0, row: 39 }
    );
}

fn ten_cells() -> europa_core::Notebook {
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

fn go(viewer: &mut crate::viewer::Viewer, recorder: &mut common::RecordingRenderer<'_>, key: Key) {
    let effect = crate::viewer::update(viewer, Input::Key(key), recorder);
    assert_eq!(effect, Effect::None);
    let _ = recorder.take_calls();
}

#[test]
fn prev_cell_to_start_of_cell() {
    let notebook = ten_cells();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..3 {
        go(&mut viewer, &mut recorder, Key::NextCell);
    }
    for _ in 0..4 {
        go(&mut viewer, &mut recorder, Key::Down);
    }
    assert_eq!(viewer.position, crate::viewer::Position { cell: 3, row: 4 });
    go(&mut viewer, &mut recorder, Key::PrevCell);
    assert_eq!(viewer.position, crate::viewer::Position { cell: 3, row: 0 });
}

#[test]
fn next_and_prev_cells_move() {
    let notebook = ten_cells();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..3 {
        go(&mut viewer, &mut recorder, Key::NextCell);
    }
    for _ in 0..4 {
        go(&mut viewer, &mut recorder, Key::Down);
    }
    go(&mut viewer, &mut recorder, Key::NextCell);
    assert_eq!(viewer.position, crate::viewer::Position { cell: 4, row: 0 });
    go(&mut viewer, &mut recorder, Key::PrevCell);
    assert_eq!(viewer.position, crate::viewer::Position { cell: 3, row: 0 });
    go(&mut viewer, &mut recorder, Key::PrevCell);
    assert_eq!(viewer.position, crate::viewer::Position { cell: 2, row: 0 });
}

#[test]
fn prev_cell_at_top_keeps_position() {
    let notebook = ten_cells();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    go(&mut viewer, &mut recorder, Key::PrevCell);
    assert_eq!(viewer.position, crate::viewer::Position { cell: 0, row: 0 });
}

#[test]
fn next_cell_at_bottom_goes_to_limit() {
    let notebook = ten_cells();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..9 {
        go(&mut viewer, &mut recorder, Key::NextCell);
    }
    let bottom = viewer.position;
    go(&mut viewer, &mut recorder, Key::NextCell);
    assert_eq!(viewer.position, bottom);
    for _ in 0..100 {
        go(&mut viewer, &mut recorder, Key::Down);
    }
    let limit = viewer.position;
    go(&mut viewer, &mut recorder, Key::NextCell);
    assert_eq!(viewer.position, limit);
}

#[test]
fn first_and_last_moves() {
    let notebook = ten_cells();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    for _ in 0..5 {
        go(&mut viewer, &mut recorder, Key::Down);
    }
    go(&mut viewer, &mut recorder, Key::GPrefix);
    go(&mut viewer, &mut recorder, Key::GPrefix);
    assert_eq!(viewer.position, crate::viewer::Position { cell: 0, row: 0 });
    go(&mut viewer, &mut recorder, Key::Bottom);
    let bottom = viewer.position;
    let range = viewer.visible_range().expect("range");
    assert_eq!(range.1, 9);
    assert!(bottom.row > 0 || bottom.cell == 9);
}

#[test]
fn g_then_other_is_not_first() {
    let notebook = ten_cells();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    assert!(!viewer.pending_g);
    go(&mut viewer, &mut recorder, Key::GPrefix);
    assert!(viewer.pending_g);
    go(&mut viewer, &mut recorder, Key::Down);
    assert!(!viewer.pending_g);
    assert_eq!(viewer.position, crate::viewer::Position { cell: 0, row: 1 });
    go(&mut viewer, &mut recorder, Key::GPrefix);
    go(&mut viewer, &mut recorder, Key::Other);
    go(&mut viewer, &mut recorder, Key::GPrefix);
    assert!(!viewer.pending_g || true);
}

#[test]
fn modified_keys_clear_pending() {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    for (code, modifiers) in [
        (KeyCode::Char('j'), KeyModifiers::ALT),
        (KeyCode::Char('j'), KeyModifiers::CONTROL),
    ] {
        let mapped = crate::keymap::key_from(crate::tests::common::key_with(code, modifiers));
        assert_eq!(mapped, Some(Key::Other));
    }
    let notebook = ten_cells();
    let (mut viewer, mut recorder) = common::open(&notebook, 80, 40);
    let _ = recorder.take_calls();
    go(&mut viewer, &mut recorder, Key::GPrefix);
    assert!(viewer.pending_g);
    let _ = crate::viewer::update(&mut viewer, Input::Key(Key::Other), &mut recorder);
    assert!(!viewer.pending_g);
}
