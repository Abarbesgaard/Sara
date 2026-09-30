use super::*;
use crate::test_support::{key, render_to_string};

fn ctx_with_deps() -> FormContext {
    FormContext {
        initial: FormInput {
            description: "test".into(),
            project: "tk".into(),
            priority: Some(Priority::L),
            due: "today".into(),
            tags: "a,b".into(),
            selected_deps: vec![],
            selected_files: vec![],
        },
        available_deps: vec![("1".into(), "jshfgklfhg".into())],
        available_files: vec!["Cargo.toml".into(), "README.md".into()],
        suggested_dep_indices: vec![],
        suggested_files: vec![],
    }
}

#[test]
#[ignore = "capture helper: run with --ignored to (re)write snapshot files"]
fn write_render_snapshots() {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/infrastructure/tui/snapshots"
    );
    std::fs::create_dir_all(dir).unwrap();

    let mut state = FormState::new(ctx_with_deps());
    let out = render_to_string(120, 40, |f| render(f, &mut state));
    std::fs::write(format!("{dir}/review_form_normal.txt"), out).unwrap();

    let mut state = FormState::new(ctx_with_deps());
    let out = render_to_string(40, 10, |f| render(f, &mut state));
    std::fs::write(format!("{dir}/review_form_small.txt"), out).unwrap();
}

#[test]
fn render_normal_matches_snapshot() {
    let mut state = FormState::new(ctx_with_deps());
    let out = render_to_string(120, 40, |f| render(f, &mut state));
    assert_eq!(
        out,
        include_str!("snapshots/review_form_normal.txt").replace("\r\n", "\n"),
    );
}

#[test]
fn render_small_terminal_matches_snapshot() {
    let mut state = FormState::new(ctx_with_deps());
    let out = render_to_string(40, 10, |f| render(f, &mut state));
    assert_eq!(
        out,
        include_str!("snapshots/review_form_small.txt").replace("\r\n", "\n"),
    );
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
    assert_eq!(state.focus, Focus::Tags);
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
