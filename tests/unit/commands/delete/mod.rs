use super::*;
use crate::infrastructure::model::{Status, Task};

fn seed(conn: &Connection, desc: &str) -> Task {
    let mut task = Task::new(desc.into(), "proj".into());
    db::insert_task(conn, &mut task).unwrap();
    task
}

#[test]
fn delete_soft_deletes_the_task() {
    let conn = db::open_in_memory_for_test();
    let task = seed(&conn, "throwaway");

    run(&conn, "1", true).unwrap();

    let stored = db::get_task_by_uuid_prefix(&conn, &task.uuid.to_string())
        .unwrap()
        .expect("task row should still exist after a soft delete");
    assert_eq!(
        stored.status,
        Status::Deleted,
        "delete must mark the task as Deleted, not physically remove it"
    );
    assert!(stored.end.is_some(), "delete must stamp an end timestamp");
    assert!(
        db::get_task_by_id(&conn, 1).unwrap().is_none(),
        "a deleted task must no longer resolve as a pending display id"
    );
}

#[test]
fn delete_unblocks_dependents() {
    let conn = db::open_in_memory_for_test();
    let blocker = seed(&conn, "blocker");
    let dependent = seed(&conn, "dependent");
    db::add_dependency(&conn, &dependent.uuid, &blocker.uuid).unwrap();
    assert_eq!(
        db::get_blockers(&conn, &dependent.uuid).unwrap(),
        vec![blocker.uuid],
        "precondition: the dependent is blocked"
    );

    run(&conn, &blocker.uuid.to_string(), true).unwrap();

    assert!(
        db::get_blockers(&conn, &dependent.uuid).unwrap().is_empty(),
        "deleting the blocker must unblock its dependent"
    );
}

#[test]
fn delete_repacks_remaining_display_ids() {
    let conn = db::open_in_memory_for_test();
    seed(&conn, "first");
    seed(&conn, "second");
    let third = seed(&conn, "third");

    run(&conn, "2", true).unwrap();

    let promoted = db::get_task_by_id(&conn, 2)
        .unwrap()
        .expect("a pending task should occupy id 2 after repacking");
    assert_eq!(
        promoted.uuid, third.uuid,
        "the surviving tasks must be renumbered contiguously after a delete"
    );
    assert!(
        db::get_task_by_id(&conn, 3).unwrap().is_none(),
        "no gap should remain at the old highest id"
    );
}
