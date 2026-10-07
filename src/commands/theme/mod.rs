use anyhow::{Result, bail};

use crate::infrastructure::config::{self, Config};
use crate::infrastructure::tui::theme::Palette;

pub fn run(cfg: &mut Config, chosen: Option<&str>) -> Result<String> {
    let Some(name) = chosen else {
        return Ok(status(cfg));
    };
    if Palette::parse(name).is_none() {
        bail!("unknown theme `{name}` (use classic or retro)");
    }
    cfg.tui.theme = name.trim().to_ascii_lowercase();
    config::save(cfg)?;
    Ok(format!("Theme set to {}.{}", cfg.tui.theme, env_note()))
}

pub(super) fn status(cfg: &Config) -> String {
    let saved = Palette::parse(&cfg.tui.theme).unwrap_or_default();
    let mark = |p: Palette| if p == saved { "▸" } else { " " };
    format!(
        "{} classic\n{} retro{}",
        mark(Palette::Classic),
        mark(Palette::Retro),
        env_note()
    )
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
