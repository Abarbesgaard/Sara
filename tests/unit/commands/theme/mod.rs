use super::*;

#[test]
fn theme_status_marks_the_saved_palette() {
    let mut env = crate::test_support::env_guard();
    env.remove("SARA_THEME");
    let mut cfg = Config::default();
    assert_eq!(status(&cfg), "▸ classic\n  retro");
    cfg.tui.theme = "retro".into();
    assert_eq!(status(&cfg), "  classic\n▸ retro");
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
