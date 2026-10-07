use super::*;
use crate::test_support::key;
use crossterm::event::KeyCode;

#[test]
fn activity_closes_on_quit_escape_and_enter_only() {
    let mut keys = KeyDispatcher::new();
    for c in [KeyCode::Char('q'), KeyCode::Esc, KeyCode::Enter] {
        assert!(closes(keys.dispatch(key(c), Mode::Normal)), "{c:?}");
    }
    for c in [KeyCode::Char('j'), KeyCode::Char('x'), KeyCode::Tab] {
        assert!(!closes(keys.dispatch(key(c), Mode::Normal)), "{c:?}");
    }
}
