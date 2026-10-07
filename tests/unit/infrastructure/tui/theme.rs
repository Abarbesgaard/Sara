use super::*;

const TONES: [Tone; 6] = [
    Tone::Muted,
    Tone::Accent,
    Tone::Ok,
    Tone::Warn,
    Tone::Err,
    Tone::Focus,
];

#[test]
fn theme_no_color_uses_no_colors_at_all() {
    let t = Theme::new(false);
    for tone in TONES {
        for s in [t.tone(tone), t.badge(tone)] {
            assert_eq!(s.fg, None, "{tone:?} sets fg");
            assert_eq!(s.bg, None, "{tone:?} sets bg");
        }
    }
}

#[test]
fn theme_no_color_still_distinguishes_every_tone() {
    let t = Theme::new(false);
    let styles: Vec<_> = TONES.iter().map(|&x| t.tone(x)).collect();
    for (i, a) in styles.iter().enumerate() {
        for b in &styles[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

#[test]
fn theme_color_gives_each_tone_its_own_colour() {
    let t = Theme::new(true);
    assert_eq!(t.ok().fg, Some(Color::Green));
    assert_eq!(t.err().fg, Some(Color::Red));
    assert_eq!(t.warn().fg, Some(Color::Yellow));
    assert_eq!(t.badge(Tone::Ok).bg, Some(Color::Green));
}

#[test]
fn theme_glyphs_are_single_cell() {
    for g in [
        GLYPH_DONE,
        GLYPH_OPEN,
        GLYPH_CURRENT,
        GLYPH_PROMPT,
        RAIL_LINK,
    ] {
        assert_eq!(ratatui::text::Span::raw(g).width(), 1, "{g}");
    }
}
