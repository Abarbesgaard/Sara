//! Integration tests for `sara_tasks::infrastructure::tui::review_form`.
//! Moved out of an inline mod tests block in src/infrastructure/tui/review_form.rs.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use sara_tasks::infrastructure::model::Priority;
use sara_tasks::infrastructure::tui::review_form::*;

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

/// Serialize a rendered TestBackend buffer into text: one line per row,
/// trailing whitespace trimmed, rows joined by '\n'. Captures the visible
/// glyphs (layout + labels) without styling — enough to pin the layout.
fn buffer_to_string(terminal: &Terminal<TestBackend>) -> String {
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let mut out = String::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
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
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| render(f, &mut state)).unwrap();
    std::fs::write(
        format!("{dir}/review_form_normal.txt"),
        buffer_to_string(&terminal),
    )
    .unwrap();

    let mut state = FormState::new(ctx_with_deps());
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    terminal.draw(|f| render(f, &mut state)).unwrap();
    std::fs::write(
        format!("{dir}/review_form_small.txt"),
        buffer_to_string(&terminal),
    )
    .unwrap();
}

/// Characterization (snapshot) tests for `render`. These pin the visible
/// layout and labels so the planned `render_fields` split stays behaviour-
/// identical. To intentionally update them, run:
///   cargo test write_render_snapshots -- --ignored
/// and review the diff under src/tui/snapshots/.
#[test]
fn render_normal_matches_snapshot() {
    let mut state = FormState::new(ctx_with_deps());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| render(f, &mut state)).unwrap();
    assert_eq!(
        buffer_to_string(&terminal),
        // Normalize CRLF: git may check the snapshot out with \r\n on Windows.
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/infrastructure/tui/snapshots/review_form_normal.txt"
        ))
        .replace("\r\n", "\n"),
    );
}

#[test]
fn render_small_terminal_matches_snapshot() {
    let mut state = FormState::new(ctx_with_deps());
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    terminal.draw(|f| render(f, &mut state)).unwrap();
    assert_eq!(
        buffer_to_string(&terminal),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/infrastructure/tui/snapshots/review_form_small.txt"
        ))
        .replace("\r\n", "\n"),
    );
}

#[test]
fn render_with_toggled_dep_does_not_panic() {
    let mut state = FormState::new(ctx_with_deps());
    state.focus = Focus::Dependencies;
    state.toggle_dep();
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render(f, &mut state)).unwrap();
}

#[test]
fn render_with_toggled_file_does_not_panic() {
    let mut state = FormState::new(ctx_with_deps());
    state.focus = Focus::Files;
    state.toggle_file();
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render(f, &mut state)).unwrap();
}

#[test]
fn render_small_terminal_does_not_panic() {
    let mut state = FormState::new(ctx_with_deps());
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render(f, &mut state)).unwrap();
}

// ── Key handling ──────────────────────────────────────────────────────────

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn tab_to(state: &mut FormState, target: Focus) {
    // Tab forward at most one full cycle to reach `target`.
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
    // Wrap back to the start.
    state.handle_key(key(KeyCode::Tab));
    assert_eq!(state.focus, Focus::Description);
}

/// The reported softlock: toggle a dependency with Space, then confirm
/// every subsequent key still moves focus (i.e. the form is not wedged).
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

    // Now navigation must still work.
    state.handle_key(key(KeyCode::Tab));
    assert_eq!(state.focus, Focus::Files);
    state.handle_key(key(KeyCode::Tab));
    assert_eq!(state.focus, Focus::Submit);
}

/// Same as above but with Enter (the user also tried Enter on deps).
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
    // Down moves within the 2-item list, focus stays on Files.
    state.handle_key(key(KeyCode::Down));
    assert_eq!(state.file_state.selected(), Some(1));
    assert_eq!(state.focus, Focus::Files);
    // Up moves back within the list, focus stays.
    state.handle_key(key(KeyCode::Up));
    assert_eq!(state.file_state.selected(), Some(0));
    assert_eq!(state.focus, Focus::Files);
}

/// The exact reported bug: Down on a single-item Dependencies list must
/// move focus to Files instead of getting stuck.
#[test]
fn down_from_single_item_dependencies_moves_to_files() {
    let mut state = FormState::new(ctx_with_deps()); // 1 dependency
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

/// At the bottom of a multi-item list, Down should leave the list.
#[test]
fn down_at_bottom_of_file_list_moves_to_next_field() {
    let mut state = FormState::new(ctx_with_deps()); // 2 files
    tab_to(&mut state, Focus::Files);
    state.handle_key(key(KeyCode::Down)); // -> index 1 (last)
    assert_eq!(state.focus, Focus::Files);
    state.handle_key(key(KeyCode::Down)); // at bottom -> leave
    assert_eq!(state.focus, Focus::Submit);
}

#[test]
fn down_then_up_round_trips_dependencies_and_files() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Dependencies);
    state.handle_key(key(KeyCode::Down)); // deps -> files (single dep)
    assert_eq!(state.focus, Focus::Files);
    state.handle_key(key(KeyCode::Up)); // files top -> back to deps
    assert_eq!(state.focus, Focus::Dependencies);
}

#[test]
fn toggle_second_file_via_navigation_then_space() {
    let mut state = FormState::new(ctx_with_deps());
    tab_to(&mut state, Focus::Files);
    state.handle_key(key(KeyCode::Down)); // highlight README.md (row 1)
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
    // Type "read" -> only README.md matches.
    for c in "read".chars() {
        state.handle_key(key(KeyCode::Char(c)));
    }
    let rows = state.file_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].path, "README.md");
    assert!(!rows[0].add_custom);
    // Toggle the single match on.
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
    // No project file matches, so only the synthetic add row is present.
    assert_eq!(rows.len(), 1);
    assert!(rows[0].add_custom);
    assert_eq!(rows[0].path, "src/new.rs");
    // Enter adds it (no fzf in tests) and clears the filter.
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
    // Nothing toggled directly; fzf handles selection in run_form.
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

/// Full flow: toggle a dep, then reach Submit and submit with Ctrl+S.
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

/// Regression: a space inside a text field must insert a space, not be
/// swallowed by the dependency/file toggle arm.
#[test]
fn space_in_text_field_inserts_space() {
    let mut state = FormState::new(ctx_with_deps());
    assert_eq!(state.focus, Focus::Description);
    // desc_area is pre-filled with "test"; append " a b".
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
    // Down on an empty list falls straight through to the next field.
    state.handle_key(key(KeyCode::Down));
    assert_eq!(state.focus, Focus::Files);
}
