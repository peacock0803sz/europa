//! From terminal key events to viewer keys.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::viewer::Key;

/// Turn a key event into a viewer key.
pub(crate) fn key_from(event: KeyEvent) -> Option<Key> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    if matches!(event.code, KeyCode::Char('c') | KeyCode::Char('C'))
        && event.modifiers.contains(KeyModifiers::CONTROL)
    {
        return Some(Key::Quit);
    }
    if event.modifiers.contains(KeyModifiers::CONTROL)
        || event.modifiers.contains(KeyModifiers::ALT)
    {
        return Some(Key::Other);
    }
    match event.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => Some(Key::Quit),
        KeyCode::Char('j') | KeyCode::Char('J') | KeyCode::Down => Some(Key::Down),
        KeyCode::Char('k') | KeyCode::Char('K') | KeyCode::Up => Some(Key::Up),
        KeyCode::Char(']') => Some(Key::NextCell),
        KeyCode::Char('[') => Some(Key::PrevCell),
        KeyCode::Char('g') => Some(Key::GPrefix),
        KeyCode::Char('G') => Some(Key::Bottom),
        _ => Some(Key::Other),
    }
}
