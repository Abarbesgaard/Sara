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
