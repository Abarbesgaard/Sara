use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    Up,
    Down,
    Top,
    Bottom,
    PageUp,
    PageDown,
    Confirm,
    Cancel,
    Save,
    NextFocus,
    PrevFocus,
    ToggleMark,
    ReorderUp,
    ReorderDown,
    ExternalEdit,
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

    pub const CONTROL_ACTION_BINDINGS: &[(&str, &str)] = &[
        ("Esc", "cancel"),
        ("Ctrl+S", "submit"),
        ("Tab / Shift+Tab", "next / previous field"),
    ];
}

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
