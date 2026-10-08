use super::*;
use crate::test_support::key;
use chrono::Duration;
use std::cell::RefCell;
use std::collections::HashMap;

fn state(today: NaiveDate) -> ActivityState {
    ActivityState::new(
        ActivityData {
            counts: HashMap::new(),
            project: None,
            total_created: 0,
            total_completed: 0,
            cur_streak: 0,
            longest_streak: 0,
        },
        today,
    )
}

fn task(title: &str) -> DayTask {
    DayTask {
        id: Some(1),
        title: title.into(),
        project: "demo".into(),
        status: "pending".into(),
    }
}

#[test]
fn activity_cursor_moves_by_day_and_week_and_stays_in_range() {
    let today = NaiveDate::from_ymd_opt(2026, 3, 18).unwrap();
    let mut st = state(today);
    let mut keys = KeyDispatcher::new();
    let mut load: DayLoader = Box::new(|_| Vec::new());
    let mut press = |st: &mut ActivityState, c: KeyCode| {
        apply(st, keys.dispatch(key(c), Mode::Normal), &mut load)
    };
    press(&mut st, KeyCode::Char('j'));
    assert_eq!(st.cursor, today, "cannot move past today");
    press(&mut st, KeyCode::Char('k'));
    assert_eq!(st.cursor, today - Duration::days(1));
    press(&mut st, KeyCode::Char('h'));
    press(&mut st, KeyCode::Left);
    assert_eq!(st.cursor, today - Duration::days(15));
    press(&mut st, KeyCode::Char('l'));
    assert_eq!(st.cursor, today - Duration::days(8));
    for _ in 0..80 {
        press(&mut st, KeyCode::Char('h'));
    }
    assert_eq!(st.cursor, today - Duration::days(types::HISTORY_DAYS));
    press(&mut st, KeyCode::Char('G'));
    assert_eq!(st.cursor, today);
}

#[test]
fn activity_enter_drills_into_the_cursor_day_and_esc_closes_before_quitting() {
    let today = NaiveDate::from_ymd_opt(2026, 3, 18).unwrap();
    let mut st = state(today);
    let asked = RefCell::new(Vec::new());
    let mut load: DayLoader = Box::new(|d| {
        asked.borrow_mut().push(d);
        vec![task("one")]
    });
    let mut keys = KeyDispatcher::new();
    let mut press = |st: &mut ActivityState, c: KeyCode| {
        apply(st, keys.dispatch(key(c), Mode::Normal), &mut load)
    };
    assert!(!press(&mut st, KeyCode::Enter));
    assert_eq!(st.drill.as_ref().map(|d| d.day), Some(today));
    assert_eq!(st.drill.as_ref().unwrap().tasks, vec![task("one")]);
    assert!(!press(&mut st, KeyCode::Char('k')));
    assert_eq!(
        st.drill.as_ref().map(|d| d.day),
        Some(today - Duration::days(1)),
        "open drill follows the cursor"
    );
    assert!(!press(&mut st, KeyCode::Esc));
    assert!(st.drill.is_none());
    assert!(press(&mut st, KeyCode::Esc));
    assert!(press(&mut st, KeyCode::Char('q')));
    drop(load);
    assert_eq!(asked.into_inner(), [today, today - Duration::days(1)]);
}
