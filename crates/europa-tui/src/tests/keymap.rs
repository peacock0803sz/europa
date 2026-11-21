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
