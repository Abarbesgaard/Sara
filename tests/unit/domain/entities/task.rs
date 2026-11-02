use super::*;
use chrono::TimeZone;

fn ymd(y: i32, m: u32, d: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap()
}

#[test]
fn add_months_clamps_short_target_month() {
    use chrono::Datelike;
    let r = add_months(ymd(2025, 1, 31), 1);
    assert_eq!((r.year(), r.month(), r.day()), (2025, 2, 28));

    let r = add_months(ymd(2024, 1, 31), 1);
    assert_eq!((r.year(), r.month(), r.day()), (2024, 2, 29));

    let r = add_months(ymd(2025, 3, 31), 1);
    assert_eq!((r.year(), r.month(), r.day()), (2025, 4, 30));
}

#[test]
fn add_months_preserves_valid_day_and_rolls_year() {
    use chrono::Datelike;
    let r = add_months(ymd(2025, 1, 15), 1);
    assert_eq!((r.year(), r.month(), r.day()), (2025, 2, 15));

    let r = add_months(ymd(2025, 12, 15), 1);
    assert_eq!((r.year(), r.month(), r.day()), (2026, 1, 15));

    let r = add_months(ymd(2024, 2, 29), 12);
    assert_eq!((r.year(), r.month(), r.day()), (2025, 2, 28));
}

#[test]
fn complete_banks_running_time_and_closes() {
    let now = ymd(2025, 1, 1);
    let mut t = Task::new("x".into(), "p".into());
    t.time_spent = 10;
    t.started_at = Some(now - chrono::Duration::seconds(50));
    t.complete(now);
    assert_eq!(t.status, Status::Completed);
    assert_eq!(t.time_spent, 60);
    assert_eq!(t.started_at, None);
    assert_eq!((t.end, t.modified), (Some(now), now));
}

#[test]
fn delete_closes_without_touching_the_timer() {
    let now = ymd(2025, 1, 1);
    let mut t = Task::new("x".into(), "p".into());
    t.started_at = Some(now);
    t.delete(now);
    assert_eq!(t.status, Status::Deleted);
    assert_eq!(t.end, Some(now));
    assert!(t.started_at.is_some());
}

#[test]
fn next_occurrence_carries_shape_and_advances_due() {
    let now = ymd(2025, 1, 1);
    let mut t = Task::new("x".into(), "p".into());
    assert!(t.next_occurrence(now).is_none());

    t.recur = Some("weekly".into());
    t.tags = vec!["a".into()];
    t.estimate_mins = Some(30);
    let next = t.next_occurrence(now).unwrap();
    assert_eq!(next.due, Some(ymd(2025, 1, 8)));
    assert_eq!((next.tags, next.estimate_mins), (t.tags.clone(), Some(30)));
    assert_ne!(next.uuid, t.uuid);

    t.due = Some(ymd(2025, 3, 1));
    assert_eq!(t.next_occurrence(now).unwrap().due, Some(ymd(2025, 3, 8)));
}

#[test]
fn total_time_spent_adds_the_running_session_to_logged_time() {
    let mut task = Task::new("t".into(), "p".into());
    task.time_spent = 100;
    assert_eq!(task.total_time_spent(), 100);

    task.started_at = Some(Utc::now() - chrono::Duration::seconds(60));
    let total = task.total_time_spent();
    assert!((160..=165).contains(&total), "got {total}");
}
