use anyhow::{Result, bail};
use ratatui::style::Color;
use std::io::IsTerminal;

use crate::infrastructure::config::{self, Config};
use crate::infrastructure::tui::theme::{self, Palette};

pub fn run(cfg: &mut Config, chosen: Option<&str>) -> Result<String> {
    let Some(name) = chosen else {
        return Ok(status(cfg, std::io::stdout().is_terminal()));
    };
    if Palette::parse(name).is_none() {
        bail!(
            "unknown theme `{name}` (use one of: {})",
            Palette::NAMES.join(", ")
        );
    }
    cfg.tui.theme = name.trim().to_ascii_lowercase();
    config::save(cfg)?;
    Ok(format!("Theme set to {}.{}", cfg.tui.theme, env_note()))
}

pub(super) fn status(cfg: &Config, paint: bool) -> String {
    let saved = Palette::parse(&cfg.tui.theme).unwrap_or_default();
    let mark = |p: Palette| if p == saved { "▸" } else { " " };
    let rows: Vec<String> = Palette::ALL
        .into_iter()
        .map(|p| match theme::swatch(p).filter(|_| paint) {
            Some(colors) => format!("{} {:<16}{}", mark(p), p.name(), blocks(&colors)),
            None => format!("{} {}", mark(p), p.name()),
        })
        .collect();
    format!("{}{}", rows.join("\n"), env_note())
}

fn blocks(colors: &[Color]) -> String {
    colors
        .iter()
        .map(|c| format!("\x1b[{}m██\x1b[0m", sgr(*c)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn sgr(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("38;2;{r};{g};{b}"),
        Color::Indexed(n) => format!("38;5;{n}"),
        named => {
            let code = match named {
                Color::Black => 30,
                Color::Red => 31,
                Color::Green => 32,
                Color::Yellow => 33,
                Color::Blue => 34,
                Color::Magenta => 35,
                Color::Cyan => 36,
                Color::Gray => 37,
                Color::DarkGray => 90,
                Color::LightRed => 91,
                Color::LightGreen => 92,
                Color::LightYellow => 93,
                Color::LightBlue => 94,
                Color::LightMagenta => 95,
                Color::LightCyan => 96,
                Color::White => 97,
                _ => 39,
            };
            code.to_string()
        }
    }
}

fn env_note() -> String {
    match std::env::var("SARA_THEME") {
        Ok(v) if Palette::parse(&v).is_some() => {
            format!("\nNote: SARA_THEME={v} overrides this in the current shell.")
        }
        _ => String::new(),
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/theme/mod.rs"]
mod tests;
