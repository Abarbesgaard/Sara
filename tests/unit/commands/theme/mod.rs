use super::*;

#[test]
fn theme_status_marks_the_saved_palette() {
    let mut env = crate::test_support::env_guard();
    env.remove("SARA_THEME");
    let mut cfg = Config::default();
    let out = status(&cfg);
    assert!(out.starts_with("▸ classic\n  retro\n"), "{out}");
    assert_eq!(out.lines().count(), Palette::ALL.len());
    cfg.tui.theme = "tokyo-night".into();
    let out = status(&cfg);
    assert!(out.contains("▸ tokyo-night"), "{out}");
    assert!(out.contains("  classic"), "{out}");
}

#[test]
fn theme_status_notes_an_env_override() {
    let mut env = crate::test_support::env_guard();
    env.set("SARA_THEME", "retro");
    let out = status(&Config::default());
    assert!(out.contains("SARA_THEME=retro overrides"), "{out}");
}

#[test]
fn theme_rejects_unknown_names_without_saving() {
    let mut cfg = Config::default();
    assert!(run(&mut cfg, Some("neon")).is_err());
    assert_eq!(cfg.tui.theme, "classic");
}
