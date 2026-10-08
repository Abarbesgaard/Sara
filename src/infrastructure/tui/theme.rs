use ratatui::style::{Color, Modifier, Style};
use std::cell::Cell;

use super::themes;

pub const GLYPH_DONE: &str = "●";
pub const GLYPH_OPEN: &str = "○";
pub const GLYPH_CURRENT: &str = "◆";
pub const GLYPH_PROMPT: &str = "▸";
pub const RAIL_LINK: &str = "─";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Palette {
    #[default]
    Classic,
    Retro,
    Dracula,
    Nord,
    Gruvbox,
    Catppuccin,
    TokyoNight,
    SolarizedDark,
    SolarizedLight,
    OneDark,
    RosePine,
    HighContrast,
}

impl Palette {
    pub const ALL: [Palette; 12] = [
        Self::Classic,
        Self::Retro,
        Self::Dracula,
        Self::Nord,
        Self::Gruvbox,
        Self::Catppuccin,
        Self::TokyoNight,
        Self::SolarizedDark,
        Self::SolarizedLight,
        Self::OneDark,
        Self::RosePine,
        Self::HighContrast,
    ];

    pub const NAMES: [&'static str; 12] = [
        "classic",
        "retro",
        "dracula",
        "nord",
        "gruvbox",
        "catppuccin",
        "tokyo-night",
        "solarized-dark",
        "solarized-light",
        "one-dark",
        "rose-pine",
        "high-contrast",
    ];

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }

    pub fn parse(s: &str) -> Option<Self> {
        let wanted = s.trim().to_ascii_lowercase().replace(['_', ' '], "-");
        Self::ALL.into_iter().find(|p| p.name() == wanted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Text,
    Soft,
    Muted,
    Accent,
    Ok,
    Warn,
    Err,
    Info,
    Special,
    Glow,
    Base,
    Plain,
    Select,
    Header,
    Void,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Look {
    palette: Palette,
    color: bool,
    truecolor: bool,
}

thread_local! {
    static LOOK: Cell<Look> = const {
        Cell::new(Look {
            palette: Palette::Classic,
            color: true,
            truecolor: true,
        })
    };
}

pub fn init(configured: Option<&str>) {
    let chosen = std::env::var("SARA_THEME")
        .ok()
        .and_then(|v| Palette::parse(&v))
        .or_else(|| configured.and_then(Palette::parse))
        .unwrap_or_default();
    let color = std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
    let truecolor = std::env::var("COLORTERM")
        .map(|v| v.contains("truecolor") || v.contains("24bit"))
        .unwrap_or(false);
    set_look(chosen, color, truecolor);
}

pub fn set_look(palette: Palette, color: bool, truecolor: bool) {
    LOOK.with(|l| {
        l.set(Look {
            palette,
            color,
            truecolor,
        })
    });
}

pub const SWATCH: [Ink; 8] = [
    Ink::Text,
    Ink::Accent,
    Ink::Info,
    Ink::Ok,
    Ink::Warn,
    Ink::Err,
    Ink::Special,
    Ink::Glow,
];

pub fn swatch(p: Palette) -> Option<Vec<Color>> {
    let saved = LOOK.with(Cell::get);
    if !saved.color {
        return None;
    }
    set_look(p, true, saved.truecolor);
    let out = SWATCH.into_iter().map(ink).collect();
    LOOK.with(|l| l.set(saved));
    Some(out)
}

pub fn palette() -> Palette {
    LOOK.with(|l| l.get().palette)
}

pub fn ink(i: Ink) -> Color {
    let look = LOOK.with(Cell::get);
    if !look.color {
        return Color::Reset;
    }
    themes::ink(look.palette, i, look.truecolor)
}

pub fn heat(count: u32, max: u32) -> Color {
    let level = if count == 0 {
        0
    } else {
        let ratio = count as f64 / max.max(1) as f64;
        match ratio {
            r if r < 0.25 => 1,
            r if r < 0.5 => 2,
            r if r < 0.75 => 3,
            _ => 4,
        }
    };
    let look = LOOK.with(Cell::get);
    if !look.color {
        return Color::Reset;
    }
    themes::heat(look.palette, level, look.truecolor)
}

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
            Tone::Muted => Style::new().fg(ink(Ink::Muted)),
            Tone::Accent => Style::new()
                .fg(ink(Ink::Accent))
                .add_modifier(Modifier::BOLD),
            Tone::Ok => Style::new().fg(ink(Ink::Ok)),
            Tone::Warn => Style::new().fg(ink(Ink::Warn)),
            Tone::Err => Style::new().fg(ink(Ink::Err)).add_modifier(Modifier::BOLD),
            Tone::Focus => Style::new()
                .fg(ink(Ink::Base))
                .bg(ink(Ink::Accent))
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
            Tone::Muted => ink(Ink::Muted),
            Tone::Accent | Tone::Focus => ink(Ink::Accent),
            Tone::Ok => ink(Ink::Ok),
            Tone::Warn => ink(Ink::Warn),
            Tone::Err => ink(Ink::Err),
        };
        Style::new()
            .fg(ink(Ink::Base))
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/infrastructure/tui/theme.rs"]
mod tests;
