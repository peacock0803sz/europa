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
