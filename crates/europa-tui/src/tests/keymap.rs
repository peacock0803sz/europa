//! Key mapping for the first screen.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::keymap::key_from;
use crate::viewer::Key;

fn press(code: KeyCode) -> Option<Key> {
    key_from(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn quit_keys_end_the_viewer() {
    assert_eq!(press(KeyCode::Char('q')), Some(Key::Quit));
    assert_eq!(
        key_from(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Some(Key::Quit)
    );
}

#[test]
fn other_keys_do_nothing() {
    for code in [KeyCode::Char('x'), KeyCode::Enter, KeyCode::Esc] {
        assert_eq!(press(code), Some(Key::Other), "{code:?}");
    }
}

#[test]
fn release_is_ignored_but_repeat_is_read() {
    let release = KeyEvent::new_with_kind(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    );
    assert_eq!(key_from(release), None);
    let repeat =
        KeyEvent::new_with_kind(KeyCode::Char('q'), KeyModifiers::NONE, KeyEventKind::Repeat);
    assert_eq!(key_from(repeat), Some(Key::Quit));
}

#[test]
fn alt_quit_is_not_quit() {
    assert_eq!(
        key_from(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT)),
        Some(Key::Other)
    );
}

#[test]
fn movement_keys_map_to_moves() {
    assert_eq!(press(KeyCode::Char('j')), Some(Key::Down));
    assert_eq!(press(KeyCode::Down), Some(Key::Down));
    assert_eq!(press(KeyCode::Char('k')), Some(Key::Up));
    assert_eq!(press(KeyCode::Up), Some(Key::Up));
    assert_eq!(press(KeyCode::Char(']')), Some(Key::NextCell));
    assert_eq!(press(KeyCode::Char('[')), Some(Key::PrevCell));
    assert_eq!(press(KeyCode::Char('g')), Some(Key::GPrefix));
    assert_eq!(press(KeyCode::Char('G')), Some(Key::Bottom));
    assert_eq!(
        key_from(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)),
        Some(Key::Bottom)
    );
}

#[test]
fn modified_movement_keys_do_nothing() {
    for (code, modifiers) in [
        (KeyCode::Char('j'), KeyModifiers::ALT),
        (KeyCode::Char('j'), KeyModifiers::CONTROL),
        (KeyCode::Char('g'), KeyModifiers::ALT),
    ] {
        let event = crate::tests::common::key_with(code, modifiers);
        assert_eq!(key_from(event), Some(Key::Other), "{code:?} {modifiers:?}");
    }
}

#[test]
fn repeat_moves_but_release_does_not() {
    let repeat =
        KeyEvent::new_with_kind(KeyCode::Char('j'), KeyModifiers::NONE, KeyEventKind::Repeat);
    assert_eq!(key_from(repeat), Some(Key::Down));
    let release = KeyEvent::new_with_kind(
        KeyCode::Char('j'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    );
    assert_eq!(key_from(release), None);
}
