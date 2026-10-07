use super::super::{ProjectAction, ProjectListState, ProjectRow};
use super::*;
use chrono::{Duration, Utc};

#[test]
fn styled_snapshot_projects() {
    let row = |name: &str, pending: u32, done: u32, days: Option<i64>| ProjectRow {
        name: name.into(),
        goal: Some(format!("{name} goal")),
        stack: Some("rust".into()),
        pending,
        done,
        last_activity: days.map(|d| Utc::now() - Duration::days(d) - Duration::hours(1)),
    };
    let st = ProjectListState {
        rows: vec![
            row("sara", 1, 40, Some(0)),
            row("pling-backend", 0, 2, Some(3)),
            row("dream-web", 4, 0, None),
        ],
        selected: 1,
        scroll: 0,
    };
    let lines = build_lines(&st);
    insta::assert_snapshot!(crate::test_support::render_to_styled_string(90, 10, |f| {
        render(f, &st, &lines)
    }));
}

fn three() -> ProjectListState {
    let row = |name: &str| ProjectRow {
        name: name.into(),
        goal: None,
        stack: None,
        pending: 0,
        done: 0,
        last_activity: None,
    };
    ProjectListState {
        rows: vec![row("a"), row("b"), row("c")],
        selected: 0,
        scroll: 0,
    }
}

#[test]
fn projects_keys_go_through_the_shared_dispatcher() {
    use crate::infrastructure::tui::keymap::{KeyDispatcher, Mode};
    use crate::test_support::key;
    use crossterm::event::KeyCode;
    let mut st = three();
    let mut keys = KeyDispatcher::new();
    let mut press =
        |st: &mut ProjectListState, c: KeyCode| apply(st, keys.dispatch(key(c), Mode::Normal));
    assert!(press(&mut st, KeyCode::Char('j')).is_none());
    assert_eq!(st.selected, 1);
    assert!(press(&mut st, KeyCode::Char('G')).is_none());
    assert_eq!(st.selected, 2);
    press(&mut st, KeyCode::Char('g'));
    press(&mut st, KeyCode::Char('g'));
    assert_eq!(st.selected, 0);
    assert!(press(&mut st, KeyCode::Down).is_none());
    assert!(matches!(
        press(&mut st, KeyCode::Enter),
        Some(ProjectAction::Open(n)) if n == "b"
    ));
    assert!(matches!(
        press(&mut st, KeyCode::Char('q')),
        Some(ProjectAction::Quit)
    ));
    assert!(matches!(
        press(&mut st, KeyCode::Esc),
        Some(ProjectAction::Quit)
    ));
}

#[test]
fn projects_empty_list_keys_do_not_panic() {
    let mut st = three();
    st.rows.clear();
    assert!(apply(&mut st, Action::Down).is_none());
    assert!(apply(&mut st, Action::Bottom).is_none());
    assert!(apply(&mut st, Action::Confirm).is_none());
    assert_eq!(st.selected, 0);
}

#[test]
fn projects_too_small_shows_fallback() {
    let st = three();
    let lines = build_lines(&st);
    let out = crate::test_support::render_to_string(30, 5, |f| render(f, &st, &lines));
    assert!(out.contains("Terminal too small"), "{out}");
    assert!(out.contains("need 40×6, have 30×5"), "{out}");
}
