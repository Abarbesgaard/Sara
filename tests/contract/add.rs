use crate::harness::Sara;

#[impish::contract("add/A1")]
fn an_added_task_is_pending() {
    let s = Sara::new();

    s.run(&["add", "-y", "Water the plants"]);

    let list = s.run(&["list"]);
    assert!(
        list.contains("Water the plants"),
        "added task must be listed: {list}"
    );
    let status: String = s
        .db()
        .query_row(
            "SELECT status FROM tasks WHERE description = 'Water the plants'",
            [],
            |r| r.get(0),
        )
        .expect("the task is stored");
    assert_eq!(status, "pending");
}

#[impish::contract("add/A2")]
fn an_empty_description_is_refused() {
    let s = Sara::new();

    for words in [&[][..], &["   "][..], &["+onlytag"][..]] {
        let out = s
            .cmd()
            .args(["add", "-y"])
            .args(words)
            .output()
            .expect("spawn sara");
        assert!(
            !out.status.success(),
            "`sara add -y {}` must fail",
            words.join(" ")
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("description cannot be empty"),
            "stderr names the problem"
        );
    }

    let tasks: i64 = s
        .db()
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .expect("count tasks");
    assert_eq!(tasks, 0, "no task is created");
}
