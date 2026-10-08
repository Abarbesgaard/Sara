use ratatui::style::Color;

use crate::infrastructure::tui::theme::Ink;

pub(super) fn ink(i: Ink) -> Color {
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

pub(super) const HEAT: [Color; 5] = [
    Color::Rgb(22, 27, 34),
    Color::Rgb(14, 68, 41),
    Color::Rgb(0, 109, 50),
    Color::Rgb(38, 166, 65),
    Color::Rgb(57, 211, 83),
];
