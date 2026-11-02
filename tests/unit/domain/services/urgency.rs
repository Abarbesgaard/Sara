use super::*;
use crate::domain::Priority;
use chrono::{Duration, TimeZone};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap()
}

fn task() -> Task {
    let mut t = Task::new("t".into(), "inbox".into());
    t.entry = now();
    t
}

#[test]
fn urgency_due_ramps_up_over_the_final_week() {
    let w = UrgencyWeights::default();
    let mut t = task();

    t.due = Some(now() + Duration::days(14));
    assert_eq!(compute_urgency_breakdown(&t, &w, false, 0, now()).due, 0.0);

    t.due = Some(now() + Duration::hours(84));
    assert_eq!(
        compute_urgency_breakdown(&t, &w, false, 0, now()).due,
        w.due / 2.0
    );

    t.due = Some(now() - Duration::days(1));
    assert_eq!(
        compute_urgency_breakdown(&t, &w, false, 0, now()).due,
        w.due
    );
}

#[test]
fn urgency_age_is_capped_at_age_max() {
    let w = UrgencyWeights::default();
    let mut t = task();
    t.entry = now() - Duration::days(10_000);
    assert_eq!(
        compute_urgency_breakdown(&t, &w, false, 0, now()).age,
        w.age
    );
}

#[test]
fn urgency_score_is_the_sum_of_its_breakdown() {
    let w = UrgencyWeights::default();
    let mut t = task();
    t.project = "sara".into();
    t.priority = Some(Priority::H);
    t.tags = vec!["x".into()];
    t.due = Some(now() + Duration::days(2));
    t.entry = now() - Duration::days(30);

    let b = compute_urgency_breakdown(&t, &w, true, 2, now());
    assert_eq!(b.blocking, w.blocking);
    assert_eq!(b.blocked, w.blocked);
    assert_eq!(b.project, w.project);
    assert_eq!(b.tags, w.has_tags);
    assert_eq!(compute_urgency(&t, &w, true, 2, now()), b.total());
}
