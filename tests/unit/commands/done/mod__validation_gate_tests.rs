use super::*;
use crate::infrastructure::model::Task;

fn task_with_criterion(conn: &Connection) -> Task {
    let mut task = Task::new("prove me".into(), "proj".into());
    db::insert_task(conn, &mut task).unwrap();
    db::add_step(
        conn,
        &task.uuid,
        "build is green",
        None,
        db::STEP_KIND_ACCEPTANCE,
        "human",
        Some("true"),
    )
    .unwrap();
    task
}

#[test]
fn done_refuses_unvalidated_task_with_acceptance_criteria() {
    let conn = db::open_in_memory_for_test();
    let task = task_with_criterion(&conn);
    let err = done_value(&conn, &Config::default(), &task.uuid.to_string(), false)
        .expect_err("unvalidated task with criteria must be refused");
    assert!(
        err.to_string().contains("not validated"),
        "error should name the missing validation: {err}"
    );
    let still = db::resolve_task(&conn, &task.uuid.to_string()).unwrap();
    assert_ne!(still.status, Status::Completed, "refused task stays open");
}

#[test]
fn force_closes_unvalidated_task_with_a_note() {
    let conn = db::open_in_memory_for_test();
    let task = task_with_criterion(&conn);
    let v = done_value(&conn, &Config::default(), &task.uuid.to_string(), true).unwrap();
    assert_eq!(v["status"], "completed");
    assert!(
        v["validation"]
            .as_str()
            .unwrap_or_default()
            .contains("forced"),
        "forced close records an advisory: {}",
        v["validation"]
    );
}

#[test]
fn validated_task_closes_cleanly() {
    let conn = db::open_in_memory_for_test();
    let task = task_with_criterion(&conn);
    db::set_validated(&conn, &task.uuid, "deadbeef").unwrap();
    let v = done_value(&conn, &Config::default(), &task.uuid.to_string(), false).unwrap();
    assert_eq!(v["status"], "completed");
    assert_eq!(v["validation"], Value::Null, "clean close has no advisory");
}

#[test]
fn task_without_criteria_closes_with_advisory() {
    let conn = db::open_in_memory_for_test();
    let mut task = Task::new("no dod".into(), "proj".into());
    db::insert_task(&conn, &mut task).unwrap();
    let v = done_value(&conn, &Config::default(), &task.uuid.to_string(), false).unwrap();
    assert_eq!(v["status"], "completed");
    assert!(
        v["validation"]
            .as_str()
            .unwrap_or_default()
            .contains("no acceptance criteria"),
        "missing definition of done is surfaced: {}",
        v["validation"]
    );
}
