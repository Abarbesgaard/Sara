use ratatui::style::{Color, Modifier, Style};

pub const GLYPH_DONE: &str = "●";
pub const GLYPH_OPEN: &str = "○";
pub const GLYPH_CURRENT: &str = "◆";
pub const GLYPH_PROMPT: &str = "▸";
pub const RAIL_LINK: &str = "─";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Accent,
    Ok,
    Warn,
    Err,
    Focus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub color: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self::detect()
    }
}

impl Theme {
    pub fn new(color: bool) -> Self {
        Self { color }
    }

    pub fn detect() -> Self {
        Self::new(std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()))
    }

    pub fn tone(&self, tone: Tone) -> Style {
        if !self.color {
            return match tone {
                Tone::Muted => Style::new().add_modifier(Modifier::DIM),
                Tone::Accent => Style::new().add_modifier(Modifier::BOLD),
                Tone::Ok => Style::new(),
                Tone::Warn => Style::new().add_modifier(Modifier::BOLD | Modifier::ITALIC),
                Tone::Err => Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                Tone::Focus => Style::new().add_modifier(Modifier::REVERSED),
            };
        }
        match tone {
            Tone::Muted => Style::new().fg(Color::DarkGray),
            Tone::Accent => Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            Tone::Ok => Style::new().fg(Color::Green),
            Tone::Warn => Style::new().fg(Color::Yellow),
            Tone::Err => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            Tone::Focus => Style::new()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        }
    }

    pub fn muted(&self) -> Style {
        self.tone(Tone::Muted)
    }

    pub fn accent(&self) -> Style {
        self.tone(Tone::Accent)
    }

    pub fn ok(&self) -> Style {
        self.tone(Tone::Ok)
    }

    pub fn warn(&self) -> Style {
        self.tone(Tone::Warn)
    }

    pub fn err(&self) -> Style {
        self.tone(Tone::Err)
    }

    pub fn focus(&self) -> Style {
        self.tone(Tone::Focus)
    }

    pub fn badge(&self, tone: Tone) -> Style {
        if !self.color {
            return self.tone(tone).add_modifier(Modifier::REVERSED);
        }
        let bg = match tone {
            Tone::Muted => Color::DarkGray,
            Tone::Accent | Tone::Focus => Color::Cyan,
            Tone::Ok => Color::Green,
            Tone::Warn => Color::Yellow,
            Tone::Err => Color::Red,
        };
        Style::new()
            .fg(Color::Black)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/infrastructure/tui/theme.rs"]
mod tests;
