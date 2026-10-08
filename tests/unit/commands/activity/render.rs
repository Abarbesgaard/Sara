use super::super::types::{ActivityData, ActivityState, Drill};
use super::*;
use crate::infrastructure::db::DayTask;
use std::collections::HashMap;

fn sample(project: Option<&str>) -> ActivityState {
    let today = NaiveDate::from_ymd_opt(2026, 3, 18).unwrap();
    let counts: HashMap<NaiveDate, u32> = (0..60)
        .map(|i| (today - Duration::days(i), (i % 7) as u32))
        .collect();
    let mut st = ActivityState::new(
        ActivityData {
            counts,
            project: project.map(str::to_owned),
            total_created: 120,
            total_completed: 98,
            cur_streak: 6,
            longest_streak: 21,
        },
        today,
    );
    st.cursor = today - Duration::days(2);
    st
}

#[test]
fn styled_snapshot_activity() {
    let st = sample(Some("sara"));
    insta::assert_snapshot!(crate::test_support::render_to_styled_string(90, 20, |f| {
        render(f, &st)
    }));
}

#[test]
fn styled_snapshot_activity_drill_down() {
    let mut st = sample(None);
    let day = st.cursor;
    let task = |id, title: &str, status: &str| DayTask {
        id: Some(id),
        title: title.into(),
        project: "sara".into(),
        status: status.into(),
    };
    st.drill = Some(Drill {
        day,
        tasks: vec![
            task(9, "Phase 6 projects and activity", "pending"),
            task(8, "Review form Phase 4", "completed"),
        ],
    });
    insta::assert_snapshot!(crate::test_support::render_to_styled_string(90, 20, |f| {
        render(f, &st)
    }));
}

#[test]
fn styled_snapshot_activity_retro() {
    use crate::infrastructure::tui::theme::{Palette, set_look};
    set_look(Palette::Retro, true, true);
    let st = sample(None);
    let out = crate::test_support::render_to_styled_string(90, 20, |f| render(f, &st));
    set_look(Palette::Classic, true, true);
    insta::assert_snapshot!(out);
}

#[test]
fn activity_grid_scrolls_back_to_keep_the_cursor_visible() {
    let mut st = sample(None);
    let weeks = 10;
    assert_eq!(
        grid_start(&st, weeks),
        week_start(st.today) - Duration::weeks(9)
    );
    st.cursor = st.today - Duration::weeks(30);
    assert_eq!(grid_start(&st, weeks), week_start(st.cursor));
}

#[test]
fn activity_drill_down_reports_an_empty_day() {
    let mut st = sample(Some("sara"));
    st.drill = Some(Drill {
        day: st.cursor,
        tasks: vec![],
    });
    let out = crate::test_support::render_to_string(90, 20, |f| render(f, &st));
    assert!(out.contains("Nothing touched on this day."), "{out}");
    assert!(out.contains("0 tasks touched"), "{out}");
}

#[test]
fn activity_too_small_shows_fallback() {
    let st = sample(None);
    let out = crate::test_support::render_to_string(40, 8, |f| render(f, &st));
    assert!(out.contains("Terminal too small"), "{out}");
}
