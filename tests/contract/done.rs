use crate::harness::Sara;

fn statuses(s: &Sara) -> Vec<(String, String, bool)> {
    let conn = s.db();
    let mut stmt = conn
        .prepare("SELECT description, status, end IS NOT NULL FROM tasks ORDER BY entry")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
#[impish::contract("done/A1")]
fn done_closes_exactly_the_named_task() {
    let s = Sara::new();
    s.run(&["add", "-y", "Ship it"]);
    s.run(&["add", "-y", "Keep open"]);

    let out = s.run(&["done", "1"]);

    assert!(out.contains("Done: [Demo] Ship it"), "{out}");
    assert!(
        out.contains("no acceptance criteria"),
        "closing unproven work must warn: {out}"
    );
    assert_eq!(
        statuses(&s),
        vec![
            ("Ship it".into(), "completed".into(), true),
            ("Keep open".into(), "pending".into(), false),
        ]
    );
    let list = s.run(&["list"]);
    assert!(
        !list.contains("Ship it"),
        "closed task must leave the list: {list}"
    );
    assert!(list.contains("Keep open"), "{list}");
}

fn status(s: &Sara, description: &str) -> String {
    s.db()
        .query_row(
            "SELECT status FROM tasks WHERE description = ?1",
            [description],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
#[impish::contract("done/A2")]
fn done_keeps_a_blocked_task_open() {
    let s = Sara::new();
    s.run(&["add", "-y", "Lay the foundation"]);
    s.run(&["add", "-y", "Raise the walls"]);
    s.run(&["dep", "2", "on", "1"]);

    let err = s.refused(&["done", "2"]);
    assert!(err.contains("blocked by tasks: 1"), "{err}");
    assert!(err.contains("--force"), "{err}");
    assert_eq!(status(&s, "Raise the walls"), "pending");

    s.run(&["done", "2", "--force"]);
    assert_eq!(status(&s, "Raise the walls"), "completed");
}

/// Begins a task with one acceptance criterion and returns its id.
fn begin_proven(s: &Sara, name: &str) -> String {
    let v = s.json(&["begin", name, "--check", "ok", "--verify", "true", "--json"]);
    v["acceptance"]["task"]
        .as_i64()
        .expect("task id")
        .to_string()
}

#[test]
#[impish::contract("done/A3")]
fn done_keeps_unvalidated_work_open() {
    let s = Sara::new();
    s.commit("start");
    let id = begin_proven(&s, "Unproven");

    let err = s.refused(&["done", &id]);
    assert!(err.contains("is not validated"), "{err}");
    assert!(err.contains("never validated"), "{err}");
    assert!(err.contains(&format!("validate {id}")), "{err}");
    assert_eq!(status(&s, "Unproven"), "pending");

    let out = s.run(&["done", &id, "--force"]);
    assert!(out.contains("⚠️"), "forcing unproven work must warn: {out}");
    assert_eq!(status(&s, "Unproven"), "completed");
}

#[test]
#[impish::contract("done/A3")]
fn done_closes_only_work_validated_at_head() {
    let s = Sara::new();
    s.commit("start");
    let fresh = begin_proven(&s, "Fresh");
    let stale = begin_proven(&s, "Stale");
    s.run(&["validate", &stale]);
    s.commit("later work");
    s.run(&["validate", &fresh]);

    let err = s.refused(&["done", &stale]);
    assert!(err.contains("HEAD has moved"), "{err}");
    assert_eq!(status(&s, "Stale"), "pending");

    let out = s.run(&["done", &fresh]);
    assert!(
        !out.contains("⚠️"),
        "proven work closes without a warning: {out}"
    );
    assert_eq!(status(&s, "Fresh"), "completed");
}

#[test]
#[impish::contract("done/A4")]
fn done_brings_a_recurring_task_back() {
    let s = Sara::new();
    s.run(&[
        "add",
        "-y",
        "project:garden",
        "+home",
        "pri:h",
        "Water the plants",
        "--every",
        "weekly",
    ]);
    s.run(&["modify", "1", "--due", "2026-01-05"]);
    s.run(&["add", "-y", "Sweep", "--every", "daily"]);

    let out = s.run(&["done", "1"]);
    assert!(out.contains("Next recurrence: #"), "{out}");
    assert!(out.contains("due 2026-01-12"), "{out}");
    s.run(&["done", "1"]);

    let conn = s.db();
    let pending = |description: &str| -> (String, Option<String>, String, String, String) {
        conn.query_row(
            "SELECT project, priority, tags_json, recur, date(due, 'localtime') FROM tasks
             WHERE description = ?1 AND status = 'pending'",
            [description],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .expect("the next occurrence is pending")
    };
    let (project, priority, tags, recur, due) = pending("Water the plants");
    assert_eq!(
        (
            project.as_str(),
            priority.as_deref(),
            tags.as_str(),
            recur.as_str(),
            due.as_str()
        ),
        ("garden", Some("H"), r#"["home"]"#, "weekly", "2026-01-12")
    );
    let days_ahead: f64 = conn
        .query_row(
            "SELECT julianday(due) - julianday('now') FROM tasks
             WHERE description = 'Sweep' AND status = 'pending'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        (0.99..=1.0).contains(&days_ahead),
        "no due date: one interval from now, got {days_ahead} days"
    );
}
