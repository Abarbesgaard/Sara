use super::*;
use chrono::TimeZone;

fn ymd(y: i32, m: u32, d: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap()
}

#[test]
fn add_months_clamps_short_target_month() {
    use chrono::Datelike;
    // Jan 31 + 1 month must land on Feb 28 (2025 is not a leap year),
    // not silently stay on Jan 31.
    let r = add_months(ymd(2025, 1, 31), 1);
    assert_eq!((r.year(), r.month(), r.day()), (2025, 2, 28));

    // Leap year: Feb has 29 days.
    let r = add_months(ymd(2024, 1, 31), 1);
    assert_eq!((r.year(), r.month(), r.day()), (2024, 2, 29));

    // 31-day month -> 30-day month.
    let r = add_months(ymd(2025, 3, 31), 1);
    assert_eq!((r.year(), r.month(), r.day()), (2025, 4, 30));
}

#[test]
fn add_months_preserves_valid_day_and_rolls_year() {
    use chrono::Datelike;
    let r = add_months(ymd(2025, 1, 15), 1);
    assert_eq!((r.year(), r.month(), r.day()), (2025, 2, 15));

    // Crossing the year boundary.
    let r = add_months(ymd(2025, 12, 15), 1);
    assert_eq!((r.year(), r.month(), r.day()), (2026, 1, 15));

    // +12 months (yearly), Feb 29 -> Feb 28 on a non-leap target.
    let r = add_months(ymd(2024, 2, 29), 12);
    assert_eq!((r.year(), r.month(), r.day()), (2025, 2, 28));
}
