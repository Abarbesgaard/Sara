use ratatui::style::{Color, Modifier, Style};

pub(in crate::commands::dream) const PULSE_TICKS: u64 = 40;

pub(in crate::commands::dream) fn noise(seed: u64) -> u64 {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x
}

pub(super) const NOISE_GLYPHS: &[char] =
    &['░', '▒', '·', '∙', '˙', '¸', '˚', '⁚', '⋅', '∘', '°', '~'];

pub(super) fn body_style(strength: f64, provisional: bool) -> Style {
    let mut style = if strength >= 2.0 {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else if strength >= 1.5 {
        Style::default().fg(Color::Gray)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    if provisional {
        style = style.add_modifier(Modifier::ITALIC);
    }
    style
}

pub(super) fn accent(provisional: bool) -> Color {
    if provisional {
        Color::Magenta
    } else {
        Color::Cyan
    }
}

pub(in crate::commands::dream) fn resolve_progress(frame: u64, strength: f64) -> f64 {
    let total_ticks = (100.0 - 25.0 * strength).max(30.0);
    (frame as f64 / total_ticks).min(1.0)
}

pub(in crate::commands::dream) fn materialized_body(
    body: &str,
    frame: u64,
    strength: f64,
) -> Vec<(char, bool)> {
    let chars: Vec<char> = body.chars().collect();
    let progress = resolve_progress(frame, strength);
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let jitter = (noise(i as u64) % 1000) as f64 / 1000.0;
            let threshold = 0.15 + 0.85 * jitter;
            if progress >= threshold || c == '\n' {
                (c, true)
            } else if c == ' ' {
                (' ', false)
            } else {
                let g = NOISE_GLYPHS
                    [(noise(i as u64 ^ (frame / 3)) % NOISE_GLYPHS.len() as u64) as usize];
                (g, false)
            }
        })
        .collect()
}
