use super::*;

#[test]
fn theme_status_marks_the_saved_palette() {
    let mut env = crate::test_support::env_guard();
    env.remove("SARA_THEME");
    let mut cfg = Config::default();
    let out = status(&cfg, false);
    assert!(out.starts_with("▸ classic\n  retro\n"), "{out}");
    assert_eq!(out.lines().count(), Palette::ALL.len());
    cfg.tui.theme = "tokyo-night".into();
    let out = status(&cfg, false);
    assert!(out.contains("▸ tokyo-night"), "{out}");
    assert!(out.contains("  classic"), "{out}");
}

#[test]
fn theme_status_notes_an_env_override() {
    let mut env = crate::test_support::env_guard();
    env.set("SARA_THEME", "retro");
    let out = status(&Config::default(), false);
    assert!(out.contains("SARA_THEME=retro overrides"), "{out}");
}

#[test]
fn theme_rejects_unknown_names_without_saving() {
    let mut cfg = Config::default();
    assert!(run(&mut cfg, Some("neon")).is_err());
    assert_eq!(cfg.tui.theme, "classic");
}

#[test]
fn theme_status_paints_a_swatch_per_palette() {
    let mut env = crate::test_support::env_guard();
    env.remove("SARA_THEME");
    crate::infrastructure::tui::theme::set_look(Palette::Classic, true, true);
    let out = status(&Config::default(), true);
    let nord = out.lines().find(|l| l.contains("nord")).unwrap();
    assert_eq!(nord.matches("██").count(), 8, "{nord}");
    assert!(nord.contains("\x1b[38;2;136;192;208m"), "{nord}");
    let classic = out.lines().next().unwrap();
    assert!(classic.contains("\x1b[36m██"), "{classic}");
}

#[test]
fn theme_status_stays_plain_without_colour() {
    let mut env = crate::test_support::env_guard();
    env.remove("SARA_THEME");
    crate::infrastructure::tui::theme::set_look(Palette::Classic, false, true);
    let out = status(&Config::default(), true);
    crate::infrastructure::tui::theme::set_look(Palette::Classic, true, true);
    assert!(!out.contains('\x1b'), "{out}");
}
