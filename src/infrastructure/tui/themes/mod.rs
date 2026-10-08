use ratatui::style::Color;

use super::theme::{Ink, Palette};

mod catppuccin;
mod classic;
mod dracula;
mod gruvbox;
mod high_contrast;
mod nord;
mod one_dark;
mod retro;
mod rose_pine;
mod solarized_dark;
mod solarized_light;
mod tokyo_night;

pub(super) struct Scheme {
    text: u32,
    soft: u32,
    muted: u32,
    accent: u32,
    ok: u32,
    warn: u32,
    err: u32,
    info: u32,
    special: u32,
    glow: u32,
    base: u32,
    select: u32,
    header: u32,
    void: u32,
    heat: [u32; 5],
}

impl Scheme {
    fn ink(&self, i: Ink, truecolor: bool) -> Color {
        let hex = match i {
            Ink::Text => self.text,
            Ink::Soft => self.soft,
            Ink::Muted => self.muted,
            Ink::Accent => self.accent,
            Ink::Ok => self.ok,
            Ink::Warn => self.warn,
            Ink::Err => self.err,
            Ink::Info => self.info,
            Ink::Special => self.special,
            Ink::Glow => self.glow,
            Ink::Base => self.base,
            Ink::Select => self.select,
            Ink::Header => self.header,
            Ink::Void => self.void,
            Ink::Plain => return Color::Reset,
        };
        shade(hex, truecolor)
    }

    fn heat(&self, level: usize, truecolor: bool) -> Color {
        shade(self.heat[level], truecolor)
    }
}

pub(super) fn ink(p: Palette, i: Ink, truecolor: bool) -> Color {
    match scheme(p) {
        Some(s) => s.ink(i, truecolor),
        None if p == Palette::Retro => retro::ink(i, truecolor),
        None => classic::ink(i),
    }
}

pub(super) fn heat(p: Palette, level: usize, truecolor: bool) -> Color {
    match scheme(p) {
        Some(s) => s.heat(level, truecolor),
        None if p == Palette::Retro => retro::heat(level, truecolor),
        None => classic::HEAT[level],
    }
}

fn scheme(p: Palette) -> Option<&'static Scheme> {
    Some(match p {
        Palette::Classic | Palette::Retro => return None,
        Palette::Dracula => &dracula::SCHEME,
        Palette::Nord => &nord::SCHEME,
        Palette::Gruvbox => &gruvbox::SCHEME,
        Palette::Catppuccin => &catppuccin::SCHEME,
        Palette::TokyoNight => &tokyo_night::SCHEME,
        Palette::SolarizedDark => &solarized_dark::SCHEME,
        Palette::SolarizedLight => &solarized_light::SCHEME,
        Palette::OneDark => &one_dark::SCHEME,
        Palette::RosePine => &rose_pine::SCHEME,
        Palette::HighContrast => &high_contrast::SCHEME,
    })
}

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

pub(super) fn xterm256(hex: u32) -> u8 {
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

#[cfg(test)]
#[path = "../../../../tests/unit/infrastructure/tui/themes.rs"]
mod tests;
