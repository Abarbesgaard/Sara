use super::*;
use chrono::{Datelike, Duration, NaiveDate};
use proptest::prelude::*;

proptest! {
    #[test]
    fn parse_due_never_panics(s in "\\PC{0,40}", us in any::<bool>()) {
        let _ = parse_due(&s, if us { "us" } else { "uk" });
    }

    #[test]
    fn parse_due_never_panics_on_offsets(n in any::<i64>(), unit in "[dw]") {
        let _ = parse_due(&format!("+{n}{unit}"), "uk");
    }

    #[test]
    fn parse_due_iso_date_is_end_of_that_day(y in 1i32..=9999, m in 1u32..=12, d in 1u32..=28) {
        let date = NaiveDate::from_ymd_opt(y, m, d).unwrap();
        let due = parse_due(&date.format("%Y-%m-%d").to_string(), "uk").unwrap();
        prop_assert_eq!(due.date_naive(), date);
        prop_assert_eq!(due.time(), chrono::NaiveTime::from_hms_opt(23, 59, 59).unwrap());
    }

    #[test]
    fn parse_due_day_offset_lands_n_days_ahead(n in 0i64..100_000) {
        let before = Utc::now();
        let due = parse_due(&format!("+{n}d"), "uk").unwrap();
        let after = Utc::now();
        prop_assert!(due >= before + Duration::days(n) && due <= after + Duration::days(n));
    }

    #[test]
    fn parse_due_week_offset_matches_days(n in 0i64..10_000) {
        let w = parse_due(&format!("+{n}w"), "uk").unwrap();
        let d = parse_due(&format!("+{}d", n * 7), "uk").unwrap();
        prop_assert!((w - d).num_seconds().abs() <= 1);
    }

    #[test]
    fn is_valid_due_agrees_with_parse_due(s in "\\PC{0,20}") {
        let t = s.trim();
        prop_assert_eq!(is_valid_due(&s), t.is_empty() || parse_due(t, "uk").is_some());
    }

    #[test]
    fn parse_duration_never_panics(s in "\\PC{0,30}") {
        let _ = parse_duration_mins(&s);
    }

    #[test]
    fn parse_duration_never_panics_on_large_numbers(h in any::<i64>(), m in any::<i64>()) {
        let _ = parse_duration_mins(&format!("{h}h{m}m"));
        let _ = parse_duration_mins(&format!("{h}h"));
    }

    #[test]
    fn parse_duration_hours_and_minutes(h in 0i64..10_000, m in 0i64..60, upper in any::<bool>(), space in any::<bool>()) {
        let sep = if space { " " } else { "" };
        let s = format!("{h}h{sep}{m}m");
        let s = if upper { s.to_uppercase() } else { s };
        prop_assert_eq!(parse_duration_mins(&s), Some(h * 60 + m));
    }

    #[test]
    fn parse_duration_is_never_negative(s in "\\PC{0,30}") {
        prop_assert!(parse_duration_mins(&s).is_none_or(|m| m >= 0));
    }

    #[test]
    fn parse_duration_minutes_only(m in 0i64..1_000_000) {
        prop_assert_eq!(parse_duration_mins(&format!("{m}m")), Some(m));
        prop_assert_eq!(parse_duration_mins(&m.to_string()), Some(m));
    }
}

#[test]
fn parse_due_iso_date_keeps_year_month_day() {
    let due = parse_due("2026-02-28", "uk").unwrap();
    assert_eq!((due.year(), due.month(), due.day()), (2026, 2, 28));
}

#[test]
fn parse_duration_rejects_negative_values() {
    for s in ["-30m", "-30", "-2h", "1h-30m", "-1h30m"] {
        assert_eq!(parse_duration_mins(s), None, "{s}");
    }
}
