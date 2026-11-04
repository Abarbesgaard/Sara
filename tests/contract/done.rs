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
