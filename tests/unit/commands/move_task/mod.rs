use super::*;
use crate::test_support::seed_task;

#[test]
fn move_value_rejects_empty_target_project() {
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();
    seed_task(&conn, "wanderer", "home");

    let err =
        move_value(&conn, &cfg, "1", "   ").expect_err("a blank target project must be refused");
    assert!(
        err.to_string().to_lowercase().contains("empty"),
        "error should explain the empty project: {err}"
    );
}

#[test]
fn move_value_is_a_noop_for_the_same_project() {
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();
    let task = seed_task(&conn, "stayer", "home");

    let v = move_value(&conn, &cfg, "1", "home").unwrap();
    assert_eq!(v["changed"], serde_json::json!(false));
    assert_eq!(v["from"], serde_json::json!("home"));
    assert_eq!(v["to"], serde_json::json!("home"));

    let stored = db::get_task_by_uuid_prefix(&conn, &task.uuid.to_string())
        .unwrap()
        .unwrap();
    assert_eq!(stored.project, "home");
}

#[test]
fn move_value_reassigns_the_project() {
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();
    let task = seed_task(&conn, "mover", "home");

    let v = move_value(&conn, &cfg, "1", "work").unwrap();
    assert_eq!(v["changed"], serde_json::json!(true));
    assert_eq!(v["from"], serde_json::json!("home"));
    assert_eq!(v["to"], serde_json::json!("work"));

    let stored = db::get_task_by_uuid_prefix(&conn, &task.uuid.to_string())
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.project, "work",
        "the task's project column must be updated on disk"
    );
}

#[test]
fn move_value_trims_surrounding_whitespace() {
    let conn = db::open_in_memory_for_test();
    let cfg = Config::default();
    let task = seed_task(&conn, "mover", "home");

    let v = move_value(&conn, &cfg, "1", "  work  ").unwrap();
    assert_eq!(v["to"], serde_json::json!("work"));

    let stored = db::get_task_by_uuid_prefix(&conn, &task.uuid.to_string())
        .unwrap()
        .unwrap();
    assert_eq!(stored.project, "work");
}
