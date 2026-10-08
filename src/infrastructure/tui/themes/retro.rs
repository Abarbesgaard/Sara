use ratatui::style::Color;

use crate::infrastructure::tui::theme::Ink;

pub(super) fn ink(i: Ink, truecolor: bool) -> Color {
    if truecolor {
        self::truecolor(i)
    } else {
        ansi(i)
    }
}

pub(super) fn heat(level: usize, truecolor: bool) -> Color {
    let ramp = if truecolor {
        [
            Color::Rgb(14, 26, 22),
            Color::Rgb(20, 80, 62),
            Color::Rgb(30, 140, 105),
            Color::Rgb(50, 200, 155),
            Color::Rgb(110, 255, 210),
        ]
    } else {
        [
            Color::Black,
            Color::DarkGray,
            Color::Green,
            Color::Cyan,
            Color::LightCyan,
        ]
    };
    ramp[level]
}

fn truecolor(i: Ink) -> Color {
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

fn ansi(i: Ink) -> Color {
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
