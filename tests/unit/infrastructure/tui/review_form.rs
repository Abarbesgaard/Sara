use super::*;
use crate::test_support::{key, render_to_string};

fn ctx_with_deps() -> FormContext<'static> {
    FormContext {
        initial: FormInput {
            description: "test".into(),
            project: "tk".into(),
            priority: Some(Priority::L),
            due: "today".into(),
            tags: "a,b".into(),
            ..Default::default()
        },
        available_deps: vec![("1".into(), "jshfgklfhg".into())],
        available_files: vec!["Cargo.toml".into(), "README.md".into()],
        suggested_dep_indices: vec![],
        suggested_files: vec![],
        create: true,
        lookup: None,
    }
}

fn edit_ctx() -> FormContext<'static> {
    FormContext {
        create: false,
        ..ctx_with_deps()
    }
}

fn stub_lookup(calls: std::rc::Rc<std::cell::Cell<usize>>) -> HintLookup<'static> {
    Box::new(move |q: &str| {
        calls.set(calls.get() + 1);
        vec![
            Hint {
                kind: HintKind::Task,
                label: "12".into(),
                title: format!("{q} for the API"),
            },
            Hint {
                kind: HintKind::Memory,
                label: "m864".into(),
                title: "Board phase 3 patterns".into(),
            },
        ]
    })
}

#[test]
fn render_normal_matches_snapshot() {
    let mut state = FormState::new(ctx_with_deps());
    insta::assert_snapshot!(render_to_string(120, 40, |f| render(f, &mut state)));
}

#[test]
fn render_edit_mode_matches_snapshot() {
    let mut state = FormState::new(edit_ctx());
    insta::assert_snapshot!(render_to_string(100, 30, |f| render(f, &mut state)));
}

#[test]
fn render_small_terminal_matches_snapshot() {
    let mut state = FormState::new(ctx_with_deps());
    insta::assert_snapshot!(render_to_string(40, 10, |f| render(f, &mut state)));
}

#[test]
fn render_with_hints_matches_snapshot() {
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut ctx = ctx_with_deps();
    ctx.initial.description = "Login flow".into();
    ctx.lookup = Some(stub_lookup(calls));
    let mut state = FormState::new(ctx);
    state.refresh_hints(Instant::now(), true);
    insta::assert_snapshot!(render_to_string(130, 36, |f| render(f, &mut state)));
}

#[test]
fn render_with_errors_matches_snapshot() {
    let mut state = FormState::new(ctx_with_deps());
    state.verify_area = single_line("cargo test");
    state.estimate_area = single_line("soon");
    state.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    insta::assert_snapshot!(render_to_string(110, 34, |f| render(f, &mut state)));
}

#[test]
fn render_with_toggled_dep_does_not_panic() {
    let mut state = FormState::new(ctx_with_deps());
    state.focus = Focus::Dependencies;
    state.toggle_dep();
    render_to_string(120, 40, |f| render(f, &mut state));
}

#[test]
fn render_with_toggled_file_does_not_panic() {
    let mut state = FormState::new(ctx_with_deps());
    state.focus = Focus::Files;
    state.toggle_file();
    render_to_string(120, 40, |f| render(f, &mut state));
}

#[test]
fn render_small_terminal_does_not_panic() {
    let mut state = FormState::new(ctx_with_deps());
    render_to_string(40, 10, |f| render(f, &mut state));
}

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn tab_to(state: &mut FormState, target: Focus) {
    for _ in 0..ALL_FIELDS.len() + 1 {
        if state.focus == target {
            return;
        }
        state.handle_key(key(KeyCode::Tab));
    }
    panic!("never reached focus {target:?}");
}

#[test]
fn tab_cycles_through_all_fields_and_wraps() {
    let mut state = FormState::new(ctx_with_deps());
    assert_eq!(state.focus, Focus::Description);
    assert_eq!(state.fields().len(), ALL_FIELDS.len());
    for expected in ALL_FIELDS.iter().skip(1) {
        state.handle_key(key(KeyCode::Tab));
        assert_eq!(state.focus, *expected);
    }
    state.handle_key(key(KeyCode::Tab));
    assert_eq!(state.focus, Focus::Description);
}

#[test]
fn space_toggle_dep_then_tab_still_moves_focus() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Dependencies);

    state.handle_key(key(KeyCode::Char(' ')));
    assert_eq!(
        state.selected_deps,
        vec![true],
        "space should toggle dep on"
    );
    assert_eq!(
        state.focus,
        Focus::Dependencies,
        "toggle must not move focus"
    );

    state.handle_key(key(KeyCode::Tab));
    assert_eq!(state.focus, Focus::Files);
    state.handle_key(key(KeyCode::Tab));
    assert_eq!(state.focus, Focus::Submit);
}

#[test]
fn enter_toggle_dep_then_tab_still_moves_focus() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Dependencies);

    state.handle_key(key(KeyCode::Enter));
    assert_eq!(state.selected_deps, vec![true]);
    assert_eq!(state.focus, Focus::Dependencies);

    state.handle_key(key(KeyCode::Tab));
    assert_eq!(state.focus, Focus::Files);
}

#[test]
fn space_toggles_dep_on_and_off() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Dependencies);
    state.handle_key(key(KeyCode::Char(' ')));
    assert_eq!(state.selected_deps, vec![true]);
    state.handle_key(key(KeyCode::Char(' ')));
    assert_eq!(state.selected_deps, vec![false]);
}

#[test]
fn arrows_navigate_within_multi_item_file_list() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Files);
    assert_eq!(state.file_state.selected(), Some(0));
    state.handle_key(key(KeyCode::Down));
    assert_eq!(state.file_state.selected(), Some(1));
    assert_eq!(state.focus, Focus::Files);
    state.handle_key(key(KeyCode::Up));
    assert_eq!(state.file_state.selected(), Some(0));
    assert_eq!(state.focus, Focus::Files);
}

#[test]
fn down_from_single_item_dependencies_moves_to_files() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Dependencies);
    state.handle_key(key(KeyCode::Down));
    assert_eq!(state.focus, Focus::Files);
}

#[test]
fn up_from_top_of_dependencies_moves_to_previous_field() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Dependencies);
    state.handle_key(key(KeyCode::Up));
    assert_eq!(state.focus, Focus::Links);
}

#[test]
fn down_at_bottom_of_file_list_moves_to_next_field() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Files);
    state.handle_key(key(KeyCode::Down));
    assert_eq!(state.focus, Focus::Files);
    state.handle_key(key(KeyCode::Down));
    assert_eq!(state.focus, Focus::Submit);
}

#[test]
fn down_then_up_round_trips_dependencies_and_files() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Dependencies);
    state.handle_key(key(KeyCode::Down));
    assert_eq!(state.focus, Focus::Files);
    state.handle_key(key(KeyCode::Up));
    assert_eq!(state.focus, Focus::Dependencies);
}

#[test]
fn toggle_second_file_via_navigation_then_space() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Files);
    state.handle_key(key(KeyCode::Down));
    state.handle_key(key(KeyCode::Char(' ')));
    assert!(state.selected_file_paths.contains("README.md"));
    assert_eq!(
        state.collect_result().selected_files,
        vec!["README.md".to_string()]
    );
}

#[test]
fn typing_filters_file_list() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Files);
    for c in "read".chars() {
        state.handle_key(key(KeyCode::Char(c)));
    }
    let rows = state.file_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].path, "README.md");
    assert!(!rows[0].add_custom);
    state.handle_key(key(KeyCode::Char(' ')));
    assert!(state.selected_file_paths.contains("README.md"));
}

#[test]
fn backspace_edits_filter() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Files);
    for c in "xyz".chars() {
        state.handle_key(key(KeyCode::Char(c)));
    }
    state.handle_key(key(KeyCode::Backspace));
    assert_eq!(state.file_filter, "xy");
}

#[test]
fn typing_unknown_path_offers_add_custom_row() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Files);
    for c in "src/new.rs".chars() {
        state.handle_key(key(KeyCode::Char(c)));
    }
    let rows = state.file_rows();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].add_custom);
    assert_eq!(rows[0].path, "src/new.rs");
    state.handle_key(key(KeyCode::Enter));
    assert!(state.selected_file_paths.contains("src/new.rs"));
    assert_eq!(state.file_filter, "");
    assert_eq!(
        state.collect_result().selected_files,
        vec!["src/new.rs".to_string()]
    );
}

#[test]
fn enter_requests_fzf_when_available() {
    let mut state = FormState::new(ctx_with_deps());
    state.fzf_available = true;
    tab_to(&mut state, Focus::Files);
    state.handle_key(key(KeyCode::Enter));
    assert!(state.fzf_requested);
    assert!(state.selected_file_paths.is_empty());
}

#[test]
fn preselected_files_round_trip() {
    let mut ctx = ctx_with_deps();
    ctx.initial.selected_files = vec!["Cargo.toml".into()];
    ctx.suggested_files = vec!["Cargo.toml".into()];
    let state = FormState::new(ctx);
    assert_eq!(
        state.collect_result().selected_files,
        vec!["Cargo.toml".to_string()]
    );
}

#[test]
fn can_submit_after_toggling_dep() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Dependencies);
    state.handle_key(key(KeyCode::Char(' ')));
    state.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert!(state.submitted);
    assert_eq!(state.collect_result().selected_deps, vec![0]);
}

#[test]
fn esc_cancels_even_from_dependencies() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Dependencies);
    state.handle_key(key(KeyCode::Char(' ')));
    state.handle_key(key(KeyCode::Esc));
    assert!(state.cancelled);
}

#[test]
fn question_mark_opens_help_when_not_on_a_text_field() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Priority);
    assert!(!state.showing_help);
    state.handle_key(key(KeyCode::Char('?')));
    assert!(state.showing_help);
}

#[test]
fn question_mark_is_literal_text_on_description() {
    let mut state = FormState::new(ctx_with_deps());
    assert_eq!(state.focus, Focus::Description);
    state.handle_key(key(KeyCode::Char('?')));
    assert!(!state.showing_help);
    assert!(state.desc_area.lines().join("").ends_with('?'));
}

#[test]
fn any_key_dismisses_help_without_acting_on_it() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Priority);
    state.handle_key(key(KeyCode::Char('?')));
    assert!(state.showing_help);
    let priority_before = state.priority.clone();
    state.handle_key(key(KeyCode::Right));
    assert!(!state.showing_help);
    assert_eq!(state.priority, priority_before);
}

#[test]
fn space_in_text_field_inserts_space() {
    let mut state = FormState::new(ctx_with_deps());
    assert_eq!(state.focus, Focus::Description);
    state.handle_key(key(KeyCode::Char(' ')));
    state.handle_key(key(KeyCode::Char('a')));
    state.handle_key(key(KeyCode::Char(' ')));
    state.handle_key(key(KeyCode::Char('b')));
    assert_eq!(state.desc_area.lines().join(""), "test a b");
}

#[test]
fn empty_deps_list_toggle_is_noop() {
    let mut ctx = ctx_with_deps();
    ctx.available_deps = vec![];
    let mut state = FormState::new(ctx);
    tab_to(&mut state, Focus::Dependencies);
    state.handle_key(key(KeyCode::Char(' ')));
    state.handle_key(key(KeyCode::Down));
    assert_eq!(state.focus, Focus::Files);
}

#[test]
fn edit_mode_skips_guide_fields() {
    let mut state = FormState::new(edit_ctx());
    tab_to(&mut state, Focus::Estimate);
    state.handle_key(key(KeyCode::Tab));
    assert_eq!(state.focus, Focus::Links);
    assert!(!state.fields().iter().any(|f| f.is_guide()));
}

#[test]
fn guide_fields_round_trip_in_create_mode() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Assignment);
    for c in "asked".chars() {
        state.handle_key(key(KeyCode::Char(c)));
    }
    tab_to(&mut state, Focus::Acceptance);
    for c in "tests pass".chars() {
        state.handle_key(key(KeyCode::Char(c)));
    }
    state.handle_key(key(KeyCode::Tab));
    for c in "cargo test".chars() {
        state.handle_key(key(KeyCode::Char(c)));
    }
    let out = state.collect_result();
    assert_eq!(out.assignment, "asked");
    assert_eq!(out.acceptance, "tests pass");
    assert_eq!(out.verify, "cargo test");
    assert!(out.has_guide());
}

#[test]
fn edit_mode_never_returns_guide_fields() {
    let mut ctx = edit_ctx();
    ctx.initial.assignment = "stale".into();
    let state = FormState::new(ctx);
    assert_eq!(state.collect_result().assignment, "");
}

#[test]
fn ctrl_s_from_any_field_saves_when_valid() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Estimate);
    state.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert!(state.submitted);
}

#[test]
fn verify_without_acceptance_blocks_save_and_focuses_it() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Verify);
    for c in "cargo test".chars() {
        state.handle_key(key(KeyCode::Char(c)));
    }
    tab_to(&mut state, Focus::Files);
    state.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert!(!state.submitted);
    assert_eq!(state.focus, Focus::Verify);
    assert!(state.notice.as_deref().unwrap().contains("Verify"));
    state.handle_key(key(KeyCode::Tab));
    assert!(state.notice.is_none(), "any key clears the notice");
}

#[test]
fn invalid_estimate_and_links_are_reported_inline() {
    let mut state = FormState::new(ctx_with_deps());
    state.estimate_area = single_line("soon");
    state.links_area = single_line("not-a-url");
    assert_eq!(state.error(Focus::Estimate), Some("use 90m or 1h30m"));
    assert_eq!(state.error(Focus::Links), Some("expects URLs"));
    assert!(!state.can_submit());
    state.estimate_area = single_line("1h30m");
    state.links_area = single_line("https://github.com/a/b/pull/1");
    assert!(state.can_submit());
}

#[test]
fn empty_title_is_required() {
    let mut ctx = ctx_with_deps();
    ctx.initial.description = String::new();
    let mut state = FormState::new(ctx);
    assert_eq!(state.error(Focus::Description), Some("required"));
    tab_to(&mut state, Focus::Tags);
    state.handle_key(key(KeyCode::Enter));
    tab_to(&mut state, Focus::Submit);
    state.handle_key(key(KeyCode::Enter));
    assert!(!state.submitted);
    assert_eq!(state.focus, Focus::Description);
}

#[test]
fn hints_wait_for_the_debounce_and_skip_repeat_queries() {
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut ctx = ctx_with_deps();
    ctx.initial.description = String::new();
    ctx.lookup = Some(stub_lookup(calls.clone()));
    let mut state = FormState::new(ctx);

    for c in "Login".chars() {
        state.handle_key(key(KeyCode::Char(c)));
    }
    state.refresh_hints(Instant::now(), false);
    assert_eq!(calls.get(), 0, "no lookup while still typing");

    state.refresh_hints(Instant::now() + HINT_DEBOUNCE, false);
    assert_eq!(calls.get(), 1);
    assert_eq!(state.hints.len(), 2);
    assert_eq!(state.hints[0].title, "Login for the API");

    state.refresh_hints(Instant::now() + HINT_DEBOUNCE * 2, false);
    assert_eq!(calls.get(), 1, "same title is not looked up again");
}

#[test]
fn short_titles_clear_hints_without_a_lookup() {
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut ctx = ctx_with_deps();
    ctx.initial.description = "ab".into();
    ctx.lookup = Some(stub_lookup(calls.clone()));
    let mut state = FormState::new(ctx);
    state.refresh_hints(Instant::now(), true);
    assert_eq!(calls.get(), 0);
    assert!(state.hints.is_empty());
}

#[test]
fn hints_pane_hidden_on_narrow_terminals() {
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut ctx = ctx_with_deps();
    ctx.lookup = Some(stub_lookup(calls));
    let mut state = FormState::new(ctx);
    state.refresh_hints(Instant::now(), true);
    let narrow = render_to_string(90, 34, |f| render(f, &mut state));
    assert!(!narrow.contains("Similar work"), "{narrow}");
    let wide = render_to_string(120, 34, |f| render(f, &mut state));
    assert!(wide.contains("Similar work"), "{wide}");
    assert!(wide.contains("m864]"), "{wide}");
}

#[test]
fn styled_snapshot_review_form() {
    let mut state = FormState::new(ctx_with_deps());
    insta::assert_snapshot!(crate::test_support::render_to_styled_string(100, 34, |f| {
        render(f, &mut state)
    }));
}

#[test]
fn review_form_too_small_shows_fallback() {
    let mut state = FormState::new(ctx_with_deps());
    let out = render_to_string(39, 10, |f| render(f, &mut state));
    assert!(out.contains("Terminal too small"), "{out}");
}

#[test]
fn short_terminal_scrolls_to_keep_focus_visible() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Submit);
    let out = render_to_string(60, 12, |f| render(f, &mut state));
    assert!(out.contains("Create  ^S"), "{out}");
    assert!(!out.contains("1 · Task"), "{out}");
}
