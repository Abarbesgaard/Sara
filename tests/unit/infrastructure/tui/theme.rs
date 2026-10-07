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

#[test]
fn heat_zero_is_empty_cell_and_scales_with_ratio() {
    assert_eq!(heat(0, 10), Color::Rgb(22, 27, 34));
    assert_eq!(heat(1, 10), Color::Rgb(14, 68, 41));
    assert_eq!(heat(10, 10), Color::Rgb(57, 211, 83));
}

#[test]
fn palette_parses_case_insensitively() {
    assert_eq!(Palette::parse(" Retro "), Some(Palette::Retro));
    assert_eq!(Palette::parse("classic"), Some(Palette::Classic));
    assert_eq!(Palette::parse("neon"), None);
}

#[test]
fn classic_is_the_default_palette() {
    assert_eq!(palette(), Palette::Classic);
    assert_eq!(ink(Ink::Accent), Color::Cyan);
    assert_eq!(ink(Ink::Select), Color::Blue);
}

#[test]
fn retro_uses_phosphor_truecolor_and_falls_back_to_16_colours() {
    set_look(Palette::Retro, true, true);
    assert!(matches!(ink(Ink::Accent), Color::Rgb(..)));
    assert!(matches!(heat(10, 10), Color::Rgb(..)));
    set_look(Palette::Retro, true, false);
    assert_eq!(ink(Ink::Accent), Color::LightCyan);
    assert_eq!(heat(10, 10), Color::LightCyan);
    set_look(Palette::Classic, true, true);
}

#[test]
fn no_color_resets_every_ink() {
    set_look(Palette::Retro, false, true);
    assert_eq!(ink(Ink::Err), Color::Reset);
    assert_eq!(heat(5, 10), Color::Reset);
    set_look(Palette::Classic, true, true);
}

#[test]
fn init_prefers_env_over_config_and_ignores_unknown_names() {
    let mut env = crate::test_support::env_guard();
    env.remove("NO_COLOR").set("COLORTERM", "truecolor");
    env.set("SARA_THEME", "retro");
    init(Some("classic"));
    assert_eq!(palette(), Palette::Retro);
    env.remove("SARA_THEME");
    init(Some("retro"));
    assert_eq!(palette(), Palette::Retro);
    env.set("SARA_THEME", "neon");
    init(Some("bogus"));
    assert_eq!(palette(), Palette::Classic);
    set_look(Palette::Classic, true, true);
}
