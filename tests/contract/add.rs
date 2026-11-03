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
