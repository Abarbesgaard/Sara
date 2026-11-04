use crate::harness::Sara;

/// m1 is cited by a completed task; m2 is only recalled into it (ignored).
fn seed(s: &Sara) {
    s.run(&["add", "Ship the retry policy"]);
    s.run(&["check", "1", "Write the policy"]);
    s.run(&[
        "learn",
        "--tag",
        "retry",
        "retries back off exponentially with jitter",
    ]);
    s.run(&[
        "learn",
        "--force",
        "--tag",
        "retry",
        "idempotency keys make retries safe",
    ]);
    s.run(&["recall", "m2", "--task", "1"]);
    s.run(&["step", "done", "1", "1", "--used", "m1"]);
    s.run(&["done", "1"]);
}

#[test]
fn provenance_recall_shows_track_record_in_text_and_json() {
    let s = Sara::new();
    seed(&s);

    let out = s.run(&["recall", "m1"]);
    assert!(out.contains("Track record: cited in 1 task"), "{out}");

    let v = s.json(&["recall", "m2", "--json"]);
    let p = &v["keyword"][0]["provenance"];
    assert_eq!(p["recalled_in_tasks"], 1, "{v}");
    assert_eq!(p["ignored"], 1, "{v}");
    assert_eq!(p["cited"], 0, "{v}");
}

#[test]
fn provenance_memories_lists_track_record_in_text_and_json() {
    let s = Sara::new();
    seed(&s);

    let out = s.run(&["memories"]);
    assert!(
        out.lines()
            .any(|l| l.contains("m1") && l.contains("[cited in 1 task]")),
        "{out}"
    );

    let v = s.json(&["memories", "--json"]);
    let by_label = |label: &str| {
        v["memories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["label"] == label)
            .unwrap()["provenance"]
            .clone()
    };
    assert_eq!(by_label("m1")["cited"], 1, "{v}");
    assert_eq!(by_label("m2")["ignored"], 1, "{v}");
}
