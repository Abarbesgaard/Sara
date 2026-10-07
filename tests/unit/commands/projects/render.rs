use super::super::{ProjectListState, ProjectRow};
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
