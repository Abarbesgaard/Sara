//! Shared key-translation vocabulary for the ratatui screens (`board`, `info`
//! edit, `review_form`). Each screen used to hand-roll its own
//! `match key.code { ... }` block with slightly different bindings (see
//! Sara issue #34) — this module gives them one place to agree on what a key
//! means, without knowing anything about what any given screen does with it.
//!
//! Two modes: [`Mode::Normal`] (navigating — hjkl, gg/G, paging, no free text
//! entry) and [`Mode::Insert`] (typing into a focused text field). A screen
//! picks the mode for the current key based on its own state (e.g. "am I
//! editing a field right now?") and asks a [`KeyDispatcher`] to translate the
//! raw key into an [`Action`]. Anything the dispatcher doesn't recognize as a
//! shared action comes back as `Action::Raw` so the caller can still apply
//! its own domain-specific bindings (e.g. `a` to add a step) or forward the
//! key to a text widget.
//!
//! `review_form.rs` doesn't cleanly split into Normal/Insert — its text
//! fields must accept `h`/`j`/`k`/`l`/`g`/`q` as literal characters, so the
//! full vim-ish ruleset would clobber typing. It uses [`control_action`]
//! instead, which only recognizes the handful of keys that are safe to
//! intercept regardless of focus (Esc, Ctrl+S, Tab, BackTab).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Navigating: hjkl/arrows move, gg/G jump top/bottom, no free text entry.
    Normal,
    /// Typing into a focused text field: only Esc/Enter/Ctrl+S/Tab/BackTab
    /// are intercepted, everything else passes through for the widget.
    Insert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// `q` / `Esc` in Normal mode — leave the screen.
    Quit,
    Up,
    Down,
    /// `gg` in Normal mode.
    Top,
    /// `G` in Normal mode.
    Bottom,
    PageUp,
    PageDown,
    /// `Enter` — meaning depends on the screen (open/confirm/toggle).
    Confirm,
    /// `Esc` in Insert mode — leave the field without committing further
    /// input beyond what the screen already applied.
    Cancel,
    /// `Ctrl+S` — commit/save. Works in both modes.
    Save,
    NextFocus,
    PrevFocus,
    /// `Space` in Normal mode.
    ToggleMark,
    /// `K` / `Shift+Up` in Normal mode.
    ReorderUp,
    /// `J` / `Shift+Down` in Normal mode.
    ReorderDown,
    /// `Ctrl+E` — hand off the current long text field to $EDITOR instead of
    /// the in-TUI textarea. Works in Normal mode (both modes recognize it,
    /// same as Save, but only Normal-mode screens currently use it).
    ExternalEdit,
    /// Not recognized as a shared action in this mode — hand the raw key to
    /// the caller's own mode- or domain-specific handling (e.g. a text
    /// widget, or a screen-specific letter binding like `a`/`c`/`r`/`x`).
    Raw(KeyEvent),
}

fn is_ctrl_s(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn is_ctrl_e(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('e') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn is_shift(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::SHIFT)
}

/// Keys that are safe to intercept regardless of what's focused — used by
/// screens (like `review_form`) whose text fields must keep every other key
/// as literal input. Returns `None` for anything else.
pub fn control_action(key: KeyEvent) -> Option<Action> {
    if is_ctrl_s(&key) {
        return Some(Action::Save);
    }
    match key.code {
        KeyCode::Esc => Some(Action::Cancel),
        KeyCode::Tab => Some(Action::NextFocus),
        KeyCode::BackTab => Some(Action::PrevFocus),
        _ => None,
    }
}

/// Stateful translator: owns the one bit of cross-keystroke state the shared
/// vocabulary needs (whether a lone `g` is awaiting a second `g` to become
/// `gg` == Top). A screen keeps one instance for the lifetime of its event
/// loop.
#[derive(Debug, Default)]
pub struct KeyDispatcher {
    pending_g: bool,
}

impl KeyDispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn dispatch(&mut self, key: KeyEvent, mode: Mode) -> Action {
        if is_ctrl_s(&key) {
            self.pending_g = false;
            return Action::Save;
        }
        if is_ctrl_e(&key) {
            self.pending_g = false;
            return Action::ExternalEdit;
        }
        match mode {
            Mode::Normal => self.dispatch_normal(key),
            Mode::Insert => self.dispatch_insert(key),
        }
    }

    fn dispatch_normal(&mut self, key: KeyEvent) -> Action {
        let awaiting_g = std::mem::take(&mut self.pending_g);
        if awaiting_g {
            if key.code == KeyCode::Char('g') {
                return Action::Top;
            }
            // Not a second `g` — drop the pending prefix and fall through to
            // handle this key normally (matches vim: an unbound g-prefixed
            // sequence does nothing, the next key isn't swallowed).
        } else if key.code == KeyCode::Char('g') {
            self.pending_g = true;
            return Action::Raw(key);
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Char('G') => Action::Bottom,
            KeyCode::Char('K') => Action::ReorderUp,
            KeyCode::Char('J') => Action::ReorderDown,
            KeyCode::Up if is_shift(&key) => Action::ReorderUp,
            KeyCode::Down if is_shift(&key) => Action::ReorderDown,
            KeyCode::Down | KeyCode::Char('j') => Action::Down,
            KeyCode::Up | KeyCode::Char('k') => Action::Up,
            KeyCode::PageDown => Action::PageDown,
            KeyCode::PageUp => Action::PageUp,
            KeyCode::Enter => Action::Confirm,
            KeyCode::Tab => Action::NextFocus,
            KeyCode::BackTab => Action::PrevFocus,
            KeyCode::Char(' ') => Action::ToggleMark,
            _ => Action::Raw(key),
        }
    }

    fn dispatch_insert(&mut self, key: KeyEvent) -> Action {
        self.pending_g = false;
        match key.code {
            KeyCode::Esc => Action::Cancel,
            KeyCode::Enter => Action::Confirm,
            KeyCode::Tab => Action::NextFocus,
            KeyCode::BackTab => Action::PrevFocus,
            _ => Action::Raw(key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

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
        // Second key isn't 'g' — pending prefix drops, and this key still
        // gets its normal meaning (not silently eaten).
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
        // The pending 'g' from before Ctrl+E must not survive into this gg check.
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
        // Letters that mean something in Normal mode (g/q/j/k) must pass
        // through untouched here — they're being typed into a field.
        for c in ['g', 'q', 'j', 'k', ' '] {
            assert_eq!(
                d.dispatch(key(KeyCode::Char(c)), Mode::Insert),
                Action::Raw(key(KeyCode::Char(c)))
            );
        }
    }

    #[test]
    fn insert_mode_does_not_accumulate_g_state() {
        // A stray 'g' in Insert mode must never leak into a later Normal-mode
        // gg sequence (e.g. user types "g" into a field, then Esc, then gg).
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
}
