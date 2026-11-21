//! From terminal key events to viewer keys.

use ratatui::crossterm::event::KeyEvent;

use crate::viewer::Key;

/// Turn a key event into a viewer key.
pub(crate) fn key_from(event: KeyEvent) -> Option<Key> {
    let _ = event;
    todo!()
}
