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

#[test]
fn every_palette_round_trips_its_name() {
    for p in Palette::ALL {
        assert_eq!(Palette::parse(p.name()), Some(p));
    }
    assert_eq!(Palette::parse("Tokyo_Night"), Some(Palette::TokyoNight));
    assert_eq!(Palette::parse("rose pine"), Some(Palette::RosePine));
}

#[test]
fn scheme_palettes_use_truecolor_and_fall_back_to_256_colours() {
    for p in &Palette::ALL[2..] {
        set_look(*p, true, true);
        assert!(matches!(ink(Ink::Accent), Color::Rgb(..)), "{p:?}");
        assert!(matches!(heat(10, 10), Color::Rgb(..)), "{p:?}");
        assert_eq!(ink(Ink::Plain), Color::Reset, "{p:?}");
        set_look(*p, true, false);
        assert!(matches!(ink(Ink::Accent), Color::Indexed(..)), "{p:?}");
        assert!(matches!(heat(10, 10), Color::Indexed(..)), "{p:?}");
    }
    set_look(Palette::Classic, true, true);
}

fn luminance(c: Color) -> f64 {
    let Color::Rgb(r, g, b) = c else {
        panic!("expected rgb, got {c:?}")
    };
    let lin = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn contrast(a: Ink, b: Ink) -> f64 {
    let (x, y) = (luminance(ink(a)), luminance(ink(b)));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn scheme_palettes_keep_text_readable() {
    let mut weak = Vec::new();
    for p in &Palette::ALL[2..] {
        set_look(*p, true, true);
        for (fg, bg, min) in [
            (Ink::Text, Ink::Select, 4.5),
            (Ink::Text, Ink::Header, 4.5),
            (Ink::Text, Ink::Base, 4.5),
            (Ink::Soft, Ink::Base, 4.5),
            (Ink::Muted, Ink::Base, 2.5),
            (Ink::Base, Ink::Accent, 3.0),
            (Ink::Base, Ink::Ok, 3.0),
            (Ink::Base, Ink::Warn, 3.0),
            (Ink::Base, Ink::Err, 3.0),
        ] {
            let got = contrast(fg, bg);
            if got < min {
                weak.push(format!("{p:?}: {fg:?} on {bg:?} = {got:.2} < {min}"));
            }
        }
    }
    set_look(Palette::Classic, true, true);
    assert!(weak.is_empty(), "{}", weak.join("\n"));
}

#[test]
fn swatch_reads_another_palette_without_switching() {
    set_look(Palette::Classic, true, false);
    let colors = swatch(Palette::Nord).unwrap();
    assert_eq!(colors.len(), SWATCH.len());
    assert!(colors.iter().all(|c| matches!(c, Color::Indexed(..))));
    assert_eq!(palette(), Palette::Classic);
    set_look(Palette::Classic, false, true);
    assert_eq!(swatch(Palette::Nord), None);
    set_look(Palette::Classic, true, true);
}
