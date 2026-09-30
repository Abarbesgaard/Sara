use super::*;

use crate::test_support::key;

fn ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

fn shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

#[test]
fn normal_mode_hjkl_moves() {
    let mut d = KeyDispatcher::new();
    assert_eq!(
        d.dispatch(key(KeyCode::Char('j')), Mode::Normal),
        Action::Down
    );
    assert_eq!(d.dispatch(key(KeyCode::Down), Mode::Normal), Action::Down);
    assert_eq!(
        d.dispatch(key(KeyCode::Char('k')), Mode::Normal),
        Action::Up
    );
    assert_eq!(d.dispatch(key(KeyCode::Up), Mode::Normal), Action::Up);
}

#[test]
fn normal_mode_gg_is_top_but_lone_g_is_not() {
    let mut d = KeyDispatcher::new();
    assert_eq!(
        d.dispatch(key(KeyCode::Char('g')), Mode::Normal),
        Action::Raw(key(KeyCode::Char('g')))
    );
    assert_eq!(
        d.dispatch(key(KeyCode::Char('g')), Mode::Normal),
        Action::Top
    );
}

#[test]
fn normal_mode_stray_g_does_not_swallow_the_next_key() {
    let mut d = KeyDispatcher::new();
    let _ = d.dispatch(key(KeyCode::Char('g')), Mode::Normal);
    assert_eq!(
        d.dispatch(key(KeyCode::Char('j')), Mode::Normal),
        Action::Down
    );
}

#[test]
fn normal_mode_capital_g_is_bottom() {
    let mut d = KeyDispatcher::new();
    assert_eq!(
        d.dispatch(key(KeyCode::Char('G')), Mode::Normal),
        Action::Bottom
    );
}

#[test]
fn normal_mode_reorder_via_capital_letter_or_shift_arrow() {
    let mut d = KeyDispatcher::new();
    assert_eq!(
        d.dispatch(key(KeyCode::Char('K')), Mode::Normal),
        Action::ReorderUp
    );
    assert_eq!(
        d.dispatch(key(KeyCode::Char('J')), Mode::Normal),
        Action::ReorderDown
    );
    assert_eq!(
        d.dispatch(shift(KeyCode::Up), Mode::Normal),
        Action::ReorderUp
    );
    assert_eq!(
        d.dispatch(shift(KeyCode::Down), Mode::Normal),
        Action::ReorderDown
    );
}

#[test]
fn normal_mode_quit_paging_confirm_and_space() {
    let mut d = KeyDispatcher::new();
    assert_eq!(
        d.dispatch(key(KeyCode::Char('q')), Mode::Normal),
        Action::Quit
    );
    assert_eq!(d.dispatch(key(KeyCode::Esc), Mode::Normal), Action::Quit);
    assert_eq!(
        d.dispatch(key(KeyCode::PageUp), Mode::Normal),
        Action::PageUp
    );
    assert_eq!(
        d.dispatch(key(KeyCode::PageDown), Mode::Normal),
        Action::PageDown
    );
    assert_eq!(
        d.dispatch(key(KeyCode::Enter), Mode::Normal),
        Action::Confirm
    );
    assert_eq!(
        d.dispatch(key(KeyCode::Char(' ')), Mode::Normal),
        Action::ToggleMark
    );
}

#[test]
fn ctrl_s_is_save_in_either_mode() {
    let mut d = KeyDispatcher::new();
    assert_eq!(
        d.dispatch(ctrl(KeyCode::Char('s')), Mode::Normal),
        Action::Save
    );
    assert_eq!(
        d.dispatch(ctrl(KeyCode::Char('s')), Mode::Insert),
        Action::Save
    );
}

#[test]
fn ctrl_e_is_external_edit_in_either_mode_and_does_not_leak_g_state() {
    let mut d = KeyDispatcher::new();
    let _ = d.dispatch(key(KeyCode::Char('g')), Mode::Normal);
    assert_eq!(
        d.dispatch(ctrl(KeyCode::Char('e')), Mode::Normal),
        Action::ExternalEdit
    );
    assert_eq!(
        d.dispatch(key(KeyCode::Char('g')), Mode::Normal),
        Action::Raw(key(KeyCode::Char('g')))
    );
    assert_eq!(
        d.dispatch(ctrl(KeyCode::Char('e')), Mode::Insert),
        Action::ExternalEdit
    );
}

#[test]
fn plain_e_is_not_external_edit() {
    let mut d = KeyDispatcher::new();
    assert_eq!(
        d.dispatch(key(KeyCode::Char('e')), Mode::Normal),
        Action::Raw(key(KeyCode::Char('e')))
    );
}

#[test]
fn insert_mode_only_intercepts_control_keys() {
    let mut d = KeyDispatcher::new();
    assert_eq!(d.dispatch(key(KeyCode::Esc), Mode::Insert), Action::Cancel);
    assert_eq!(
        d.dispatch(key(KeyCode::Enter), Mode::Insert),
        Action::Confirm
    );
    assert_eq!(
        d.dispatch(key(KeyCode::Tab), Mode::Insert),
        Action::NextFocus
    );
    assert_eq!(
        d.dispatch(key(KeyCode::BackTab), Mode::Insert),
        Action::PrevFocus
    );
    for c in ['g', 'q', 'j', 'k', ' '] {
        assert_eq!(
            d.dispatch(key(KeyCode::Char(c)), Mode::Insert),
            Action::Raw(key(KeyCode::Char(c)))
        );
    }
}

#[test]
fn insert_mode_does_not_accumulate_g_state() {
    let mut d = KeyDispatcher::new();
    let _ = d.dispatch(key(KeyCode::Char('g')), Mode::Insert);
    assert_eq!(
        d.dispatch(key(KeyCode::Char('g')), Mode::Normal),
        Action::Raw(key(KeyCode::Char('g')))
    );
    assert_eq!(
        d.dispatch(key(KeyCode::Char('g')), Mode::Normal),
        Action::Top
    );
}

#[test]
fn control_action_recognizes_only_the_keys_safe_for_text_fields() {
    assert_eq!(control_action(key(KeyCode::Esc)), Some(Action::Cancel));
    assert_eq!(control_action(ctrl(KeyCode::Char('s'))), Some(Action::Save));
    assert_eq!(control_action(key(KeyCode::Tab)), Some(Action::NextFocus));
    assert_eq!(
        control_action(key(KeyCode::BackTab)),
        Some(Action::PrevFocus)
    );
    for c in ['h', 'j', 'k', 'l', 'g', 'q', ' '] {
        assert_eq!(control_action(key(KeyCode::Char(c))), None);
    }
    assert_eq!(control_action(key(KeyCode::Enter)), None);
}
