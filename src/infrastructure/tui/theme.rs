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
    match (look.palette, look.truecolor) {
        (Palette::Classic, _) => classic(i),
        (Palette::Retro, true) => retro(i),
        (Palette::Retro, false) => retro16(i),
        (p, truecolor) => match slot(i) {
            Some(n) => shade(scheme(p).ink[n], truecolor),
            None => Color::Reset,
        },
    }
}

fn slot(i: Ink) -> Option<usize> {
    Some(match i {
        Ink::Text => 0,
        Ink::Soft => 1,
        Ink::Muted => 2,
        Ink::Accent => 3,
        Ink::Ok => 4,
        Ink::Warn => 5,
        Ink::Err => 6,
        Ink::Info => 7,
        Ink::Special => 8,
        Ink::Glow => 9,
        Ink::Base => 10,
        Ink::Select => 11,
        Ink::Header => 12,
        Ink::Void => 13,
        Ink::Plain => return None,
    })
}

struct Scheme {
    ink: [u32; 14],
    heat: [u32; 5],
}

fn scheme(p: Palette) -> &'static Scheme {
    match p {
        Palette::Classic | Palette::Retro | Palette::Dracula => &DRACULA,
        Palette::Nord => &NORD,
        Palette::Gruvbox => &GRUVBOX,
        Palette::Catppuccin => &CATPPUCCIN,
        Palette::TokyoNight => &TOKYO_NIGHT,
        Palette::SolarizedDark => &SOLARIZED_DARK,
        Palette::SolarizedLight => &SOLARIZED_LIGHT,
        Palette::OneDark => &ONE_DARK,
        Palette::RosePine => &ROSE_PINE,
        Palette::HighContrast => &HIGH_CONTRAST,
    }
}

const DRACULA: Scheme = Scheme {
    ink: [
        0xf8f8f2, 0xbfc3d9, 0x6272a4, 0xbd93f9, 0x50fa7b, 0xffb86c, 0xff5555, 0x8be9fd, 0xff79c6,
        0xf1fa8c, 0x282a36, 0x44475a, 0x21222c, 0x191a21,
    ],
    heat: [0x21222c, 0x3b2f5a, 0x6a4fa3, 0x9a76e0, 0xbd93f9],
};

const NORD: Scheme = Scheme {
    ink: [
        0xeceff4, 0xd8dee9, 0x6b7891, 0x88c0d0, 0xa3be8c, 0xebcb8b, 0xbf616a, 0x81a1c1, 0xb48ead,
        0xd08770, 0x2e3440, 0x434c5e, 0x3b4252, 0x242933,
    ],
    heat: [0x2e3440, 0x3b5160, 0x4f7a8c, 0x6fa3b4, 0x88c0d0],
};

const GRUVBOX: Scheme = Scheme {
    ink: [
        0xebdbb2, 0xd5c4a1, 0x928374, 0xfabd2f, 0xb8bb26, 0xfe8019, 0xfb4934, 0x83a598, 0xd3869b,
        0x8ec07c, 0x282828, 0x504945, 0x3c3836, 0x1d2021,
    ],
    heat: [0x32302f, 0x4a5320, 0x79740e, 0x98971a, 0xb8bb26],
};

const CATPPUCCIN: Scheme = Scheme {
    ink: [
        0xcdd6f4, 0xbac2de, 0x7f849c, 0xcba6f7, 0xa6e3a1, 0xfab387, 0xf38ba8, 0x89b4fa, 0xf5c2e7,
        0xf9e2af, 0x1e1e2e, 0x45475a, 0x313244, 0x181825,
    ],
    heat: [0x313244, 0x3d5a45, 0x5e8a5f, 0x84b882, 0xa6e3a1],
};

const TOKYO_NIGHT: Scheme = Scheme {
    ink: [
        0xc0caf5, 0xa9b1d6, 0x565f89, 0x7aa2f7, 0x9ece6a, 0xe0af68, 0xf7768e, 0x7dcfff, 0xbb9af7,
        0xff9e64, 0x1a1b26, 0x33467c, 0x24283b, 0x16161e,
    ],
    heat: [0x1f2335, 0x26365c, 0x3d59a1, 0x5a7fd6, 0x7aa2f7],
};

const SOLARIZED_DARK: Scheme = Scheme {
    ink: [
        0xeee8d5, 0x93a1a1, 0x657b83, 0x268bd2, 0x859900, 0xb58900, 0xdc322f, 0x2aa198, 0xd33682,
        0xcb4b16, 0x002b36, 0x174652, 0x073642, 0x00212b,
    ],
    heat: [0x073642, 0x3d5200, 0x5f7300, 0x728600, 0x859900],
};

const SOLARIZED_LIGHT: Scheme = Scheme {
    ink: [
        0x073642, 0x586e75, 0x839496, 0x268bd2, 0x738a00, 0xa57c00, 0xdc322f, 0x2aa198, 0xd33682,
        0xcb4b16, 0xfdf6e3, 0xddd6c1, 0xeee8d5, 0xf5efdc,
    ],
    heat: [0xeee8d5, 0xd4dca8, 0xb3c56a, 0x93ab2c, 0x6f8500],
};

const ONE_DARK: Scheme = Scheme {
    ink: [
        0xd7dae0, 0xabb2bf, 0x7f848e, 0x61afef, 0x98c379, 0xe5c07b, 0xe06c75, 0x56b6c2, 0xc678dd,
        0xd19a66, 0x282c34, 0x3e4451, 0x2c313a, 0x21252b,
    ],
    heat: [0x2c313a, 0x3a4a35, 0x557048, 0x7a9e5f, 0x98c379],
};

const ROSE_PINE: Scheme = Scheme {
    ink: [
        0xe0def4, 0x908caa, 0x6e6a86, 0xebbcba, 0x9ccfd8, 0xf6c177, 0xeb6f92, 0x3e8fb0, 0xc4a7e7,
        0xfbd99d, 0x191724, 0x403d52, 0x26233a, 0x1f1d2e,
    ],
    heat: [0x21202e, 0x2b4155, 0x31748f, 0x5fa3b4, 0x9ccfd8],
};

const HIGH_CONTRAST: Scheme = Scheme {
    ink: [
        0xffffff, 0xe0e0e0, 0xa8a8a8, 0x00ffff, 0x00ff00, 0xffff00, 0xff4040, 0x4da6ff, 0xff66ff,
        0xffd700, 0x000000, 0x0037da, 0x1a1a1a, 0x000000,
    ],
    heat: [0x1a1a1a, 0x005f00, 0x00a000, 0x00d700, 0x00ff00],
};

fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

fn shade(hex: u32, truecolor: bool) -> Color {
    if truecolor {
        rgb(hex)
    } else {
        Color::Indexed(xterm256(hex))
    }
}

fn xterm256(hex: u32) -> u8 {
    const LEVELS: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let (r, g, b) = (
        (hex >> 16 & 0xff) as i32,
        (hex >> 8 & 0xff) as i32,
        (hex & 0xff) as i32,
    );
    let step = |v: i32| match v {
        v if v < 48 => 0,
        v if v < 115 => 1,
        v => ((v - 35) / 40) as usize,
    };
    let (qr, qg, qb) = (step(r), step(g), step(b));
    let dist = |cr: i32, cg: i32, cb: i32| (r - cr).pow(2) + (g - cg).pow(2) + (b - cb).pow(2);
    let cube = dist(LEVELS[qr], LEVELS[qg], LEVELS[qb]);
    let grey = ((r + g + b) / 3 - 3).clamp(0, 230) / 10;
    let gv = 8 + 10 * grey;
    if dist(gv, gv, gv) < cube {
        232 + grey as u8
    } else {
        16 + (36 * qr + 6 * qg + qb) as u8
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
        (p, truecolor) => return shade(scheme(p).heat[level], truecolor),
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
