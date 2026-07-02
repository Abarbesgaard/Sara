//! Integration tests for `sara_tasks::commands::projects`.
//! Moved out of an inline mod tests block in src/commands/projects/mod.rs.

use chrono::{DateTime, Utc};
use sara_tasks::commands::projects::{ProjectRow, rel_time, sort_rows, truncate};

fn row(name: &str, last: Option<DateTime<Utc>>) -> ProjectRow {
    ProjectRow {
        name: name.to_string(),
        goal: None,
        stack: None,
        pending: 0,
        done: 0,
        last_activity: last,
    }
}

#[test]
fn sort_rows_orders_by_recent_activity_then_name() {
    let now = Utc::now();
    let mut rows = vec![
        row("zeta", Some(now - chrono::Duration::days(5))),
        row("alpha", None),
        row("beta", Some(now)),
        row("gamma", None),
    ];
    sort_rows(&mut rows);
    let order: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    // beta (newest) first, then zeta (older), then None-activity by name.
    assert_eq!(order, ["beta", "zeta", "alpha", "gamma"]);
}

#[test]
fn rel_time_buckets() {
    let now = Utc::now();
    assert_eq!(rel_time(now), "just now");
    assert_eq!(rel_time(now - chrono::Duration::minutes(5)), "5m ago");
    assert_eq!(rel_time(now - chrono::Duration::hours(3)), "3h ago");
    assert_eq!(rel_time(now - chrono::Duration::days(2)), "2d ago");
}

#[test]
fn truncate_adds_ellipsis_only_when_needed() {
    assert_eq!(truncate("short", 10), "short");
    assert_eq!(truncate("abcdefgh", 4), "abc…");
}
