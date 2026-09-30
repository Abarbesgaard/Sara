use super::*;
use crate::infrastructure::db;
use crate::infrastructure::model::Task;

fn seed_task(conn: &Connection) -> Task {
    crate::test_support::seed_task(conn, "host task", "proj")
}

fn add_finding(conn: &Connection, task: &Task, text: &str) -> i64 {
    db::add_annotation_full(conn, &task.uuid, text, "finding", "ai", None, None, false).unwrap()
}

#[test]
fn surfaces_a_semantically_close_prior_finding() {
    let conn = db::open_in_memory_for_test();
    let task = seed_task(&conn);
    add_finding(
        &conn,
        &task,
        "dependabot bump broke the restore step; pin the lockfile version back",
    );
    let related = related_findings(
        &conn,
        &task.uuid,
        "the dependency update caused the restore to fail; revert the version bump",
        None,
    );
    assert!(
        !related.is_empty(),
        "a semantically adjacent prior finding must resurface"
    );
}

#[test]
fn ignores_an_unrelated_prior_finding() {
    let conn = db::open_in_memory_for_test();
    let task = seed_task(&conn);
    add_finding(&conn, &task, "how to bake sourdough bread at home");
    let related = related_findings(
        &conn,
        &task.uuid,
        "the dependency update caused the restore to fail; revert the version bump",
        None,
    );
    assert!(
        related.is_empty(),
        "an unrelated finding must not resurface (precision over recall)"
    );
}

#[test]
fn excludes_the_just_inserted_finding() {
    let conn = db::open_in_memory_for_test();
    let task = seed_task(&conn);
    let id = add_finding(
        &conn,
        &task,
        "restore broke after the dependabot version bump",
    );
    let related = related_findings(
        &conn,
        &task.uuid,
        "restore broke after the dependabot version bump",
        Some(id),
    );
    assert!(related.is_empty(), "self-match is excluded");
}

#[test]
fn only_findings_resurface_not_other_note_kinds() {
    let conn = db::open_in_memory_for_test();
    let task = seed_task(&conn);
    db::add_annotation_full(
        &conn,
        &task.uuid,
        "restore broke after the dependabot version bump",
        "comment",
        "ai",
        None,
        None,
        false,
    )
    .unwrap();
    let related = related_findings(
        &conn,
        &task.uuid,
        "restore broke after the dependabot version bump",
        None,
    );
    assert!(related.is_empty(), "comments are not findings");
}
