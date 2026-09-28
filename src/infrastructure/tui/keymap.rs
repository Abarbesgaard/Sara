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

/// Named (key label, description) pairs for the shared vocabulary, for the
/// `?` help overlay. Kept as individual constants rather than one blanket
/// list because not every screen acts on every shared `Action` (e.g. board
/// ignores `ReorderUp`/`ReorderDown` — it has nothing to reorder) — a screen
/// picks only the entries it actually handles, so the overlay never lists a
/// binding that's a silent no-op. Each pair mirrors a real arm in
/// `dispatch_normal` below; keep them in sync.
pub mod help {
    pub const MOVE: (&str, &str) = ("j/k, ↓/↑", "move down / up");
    pub const TOP_BOTTOM: (&str, &str) = ("gg / G", "jump to top / bottom");
    pub const PAGE: (&str, &str) = ("PageDown / PageUp", "scroll a page");
    pub const CONFIRM: (&str, &str) = ("Enter", "confirm / open");
    pub const QUIT: (&str, &str) = ("q / Esc", "quit / close");
    pub const SAVE: (&str, &str) = ("Ctrl+S", "save");
    pub const REORDER: (&str, &str) = ("K/J, Shift+↓/↑", "reorder");
    pub const TOGGLE_MARK: (&str, &str) = ("Space", "toggle");
    pub const HELP: (&str, &str) = ("?", "toggle this help");

    /// Mirrors `control_action`'s own arms — the subset safe to intercept in
    /// screens (like `review_form`) whose text fields need every other key
    /// as literal input.
    pub const CONTROL_ACTION_BINDINGS: &[(&str, &str)] = &[
        ("Esc", "cancel"),
        ("Ctrl+S", "submit"),
        ("Tab / Shift+Tab", "next / previous field"),
    ];
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
#[path = "../../../tests/unit/infrastructure/tui/keymap.rs"]
mod tests;
