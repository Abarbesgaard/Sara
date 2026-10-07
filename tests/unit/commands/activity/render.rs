use super::*;
use std::collections::HashMap;

#[test]
fn styled_snapshot_activity() {
    let today = NaiveDate::from_ymd_opt(2026, 3, 18).unwrap();
    let counts: HashMap<NaiveDate, u32> = (0..60)
        .map(|i| (today - Duration::days(i), (i % 7) as u32))
        .collect();
    let data = ActivityData {
        counts,
        project: Some("sara".into()),
        total_created: 120,
        total_completed: 98,
        cur_streak: 6,
        longest_streak: 21,
    };
    insta::assert_snapshot!(crate::test_support::render_to_styled_string(90, 16, |f| {
        render_on(f, &data, today)
    }));
}
