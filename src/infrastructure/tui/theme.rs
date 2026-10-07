use ratatui::style::{Color, Modifier, Style};
use std::cell::Cell;

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
}

impl Palette {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "classic" => Some(Self::Classic),
            "retro" => Some(Self::Retro),
            _ => None,
        }
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

pub fn palette() -> Palette {
    LOOK.with(|l| l.get().palette)
}

pub fn ink(i: Ink) -> Color {
    let look = LOOK.with(Cell::get);
    if !look.color {
        return Color::Reset;
    }
    match (look.palette, look.truecolor) {
        (Palette::Classic, _) => classic(i),
        (Palette::Retro, true) => retro(i),
        (Palette::Retro, false) => retro16(i),
    }
}

fn classic(i: Ink) -> Color {
    match i {
        Ink::Text => Color::White,
        Ink::Soft => Color::Gray,
        Ink::Muted => Color::DarkGray,
        Ink::Accent => Color::Cyan,
        Ink::Ok => Color::Green,
        Ink::Warn => Color::Yellow,
        Ink::Err => Color::Red,
        Ink::Info => Color::Blue,
        Ink::Special => Color::Magenta,
        Ink::Glow => Color::LightYellow,
        Ink::Base => Color::Black,
        Ink::Plain => Color::Reset,
        Ink::Select => Color::Blue,
        Ink::Header => Color::Rgb(28, 30, 44),
        Ink::Void => Color::Rgb(12, 14, 18),
    }
}

fn retro(i: Ink) -> Color {
    match i {
        Ink::Text => Color::Rgb(200, 255, 215),
        Ink::Soft => Color::Rgb(130, 205, 160),
        Ink::Muted => Color::Rgb(62, 112, 88),
        Ink::Accent => Color::Rgb(70, 245, 195),
        Ink::Ok => Color::Rgb(95, 230, 115),
        Ink::Warn => Color::Rgb(255, 190, 70),
        Ink::Err => Color::Rgb(255, 95, 85),
        Ink::Info => Color::Rgb(90, 190, 215),
        Ink::Special => Color::Rgb(190, 160, 255),
        Ink::Glow => Color::Rgb(225, 255, 160),
        Ink::Base => Color::Rgb(6, 16, 12),
        Ink::Plain => Color::Reset,
        Ink::Select => Color::Rgb(18, 72, 58),
        Ink::Header => Color::Rgb(12, 34, 28),
        Ink::Void => Color::Rgb(8, 14, 12),
    }
}

fn retro16(i: Ink) -> Color {
    match i {
        Ink::Text => Color::LightGreen,
        Ink::Soft => Color::Green,
        Ink::Muted => Color::DarkGray,
        Ink::Accent => Color::LightCyan,
        Ink::Ok => Color::Green,
        Ink::Warn => Color::Yellow,
        Ink::Err => Color::LightRed,
        Ink::Info => Color::Cyan,
        Ink::Special => Color::LightMagenta,
        Ink::Glow => Color::LightYellow,
        Ink::Base => Color::Black,
        Ink::Plain => Color::Reset,
        Ink::Select => Color::Green,
        Ink::Header => Color::Black,
        Ink::Void => Color::Black,
    }
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
    let ramp: [Color; 5] = match (look.palette, look.truecolor) {
        (Palette::Classic, _) => [
            Color::Rgb(22, 27, 34),
            Color::Rgb(14, 68, 41),
            Color::Rgb(0, 109, 50),
            Color::Rgb(38, 166, 65),
            Color::Rgb(57, 211, 83),
        ],
        (Palette::Retro, true) => [
            Color::Rgb(14, 26, 22),
            Color::Rgb(20, 80, 62),
            Color::Rgb(30, 140, 105),
            Color::Rgb(50, 200, 155),
            Color::Rgb(110, 255, 210),
        ],
        (Palette::Retro, false) => [
            Color::Black,
            Color::DarkGray,
            Color::Green,
            Color::Cyan,
            Color::LightCyan,
        ],
    };
    ramp[level]
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
