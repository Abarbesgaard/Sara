use super::*;

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
    assert_eq!(order, ["beta", "zeta", "alpha", "gamma"]);
}
