use super::*;
use crate::infrastructure::model::{Item, Status, Task};
use chrono::Utc;

#[test]
fn synthesize_done_memory_skips_when_no_steps_or_annotations() {
    let conn = mem();
    let mut task = Task::new("bare task".to_string(), "Sara".to_string());
    task.status = Status::Completed;
    insert_task(&conn, &mut task).unwrap();
    let result = synthesize_done_memory(&conn, &task.uuid, "Sara").unwrap();
    assert!(result.is_none());
}

#[test]
fn synthesize_done_memory_creates_provisional_item_with_done_steps() {
    let conn = mem();
    let mut task = Task::new("impl auth".to_string(), "Sara".to_string());
    task.tags = vec!["auth".to_string()];
    task.status = Status::Completed;
    insert_task(&conn, &mut task).unwrap();

    let step_id = add_step(
        &conn,
        &task.uuid,
        "Add login endpoint",
        None,
        STEP_KIND_STEP,
        "human",
        None,
    )
    .unwrap();
    conn.execute(
            "UPDATE task_checklist SET done=1, result='Implemented POST /login in routes.rs' WHERE id=?1",
            [step_id],
        ).unwrap();

    let label = synthesize_done_memory(&conn, &task.uuid, "Sara").unwrap();
    assert!(label.is_some(), "should create a memory");

    let listed = list_memories(&conn).unwrap();
    assert!(
        listed.iter().any(|i| i.status == "provisional"),
        "provisional memory should be listed"
    );
    let m: (String, String, String) = conn
        .query_row(
            "SELECT status, body, tags_json FROM items WHERE source_task_uuid=?1 AND kind='memory'",
            [task.uuid.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(m.0, "provisional");
    assert!(
        m.1.contains("impl auth"),
        "body should mention task description"
    );
    assert!(
        m.1.contains("login endpoint"),
        "body should mention the step"
    );
    let tags: Vec<String> = serde_json::from_str(&m.2).unwrap();
    assert!(tags.contains(&"auth".to_string()));
}

#[test]
fn synthesize_done_memory_includes_key_annotations() {
    let conn = mem();
    let mut task = Task::new("refactor db".to_string(), "Sara".to_string());
    task.status = Status::Completed;
    insert_task(&conn, &mut task).unwrap();

    let step_id = add_step(
        &conn,
        &task.uuid,
        "Move queries to db.rs",
        None,
        STEP_KIND_STEP,
        "human",
        None,
    )
    .unwrap();
    conn.execute("UPDATE task_checklist SET done=1 WHERE id=?1", [step_id])
        .unwrap();

    add_annotation_full(
        &conn,
        &task.uuid,
        "Keep all SQL in db.rs — no ORM",
        "decision",
        "human",
        None,
        None,
        false,
    )
    .unwrap();

    let label = synthesize_done_memory(&conn, &task.uuid, "Sara").unwrap();
    assert!(label.is_some());

    let body: String = conn
        .query_row(
            "SELECT body FROM items WHERE source_task_uuid=?1 AND kind='memory'",
            [task.uuid.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        body.contains("Keep all SQL in db.rs"),
        "body should include the decision annotation"
    );
}

#[test]
fn done_surfaces_review_nudge_without_archiving() {
    let conn = mem();
    let mut item = Item::new_memory("stale provisional".into(), "body".into(), None);
    item.path = Some(String::new());
    item.status = "provisional".into();
    insert_item(&conn, &mut item).unwrap();
    let old_ts = dt_to_str(&(Utc::now() - chrono::Duration::days(40)));
    conn.execute(
        "UPDATE items SET created=?1 WHERE uuid=?2",
        rusqlite::params![old_ts, item.uuid.to_string()],
    )
    .unwrap();

    let report = hygiene_pass(&conn).unwrap();
    assert!(
        report.review_pending >= 1,
        "stale provisional is counted for review"
    );
    assert!(report.oldest_age_days >= 40, "oldest age is surfaced");
    assert!(report.archived.is_empty(), "nothing lossless to archive");

    let status: String = conn
        .query_row(
            "SELECT status FROM items WHERE uuid=?1",
            rusqlite::params![item.uuid.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "provisional", "review candidate is not archived");
}

#[test]
fn archive_superseded_respects_active_superseder() {
    let conn = mem();
    let mut a1 = Item::new_memory("pair1 old".into(), "b".into(), None);
    a1.path = Some(String::new());
    insert_item(&conn, &mut a1).unwrap();
    let mut b1 = Item::new_memory("pair1 new".into(), "b".into(), None);
    b1.path = Some(String::new());
    insert_item(&conn, &mut b1).unwrap();
    insert_memory_link(
        &conn,
        &b1.uuid.to_string(),
        &a1.uuid.to_string(),
        "supersedes",
        1.0,
    )
    .unwrap();

    let mut a2 = Item::new_memory("pair2 old".into(), "b".into(), None);
    a2.path = Some(String::new());
    insert_item(&conn, &mut a2).unwrap();
    let mut b2 = Item::new_memory("pair2 new".into(), "b".into(), None);
    b2.path = Some(String::new());
    insert_item(&conn, &mut b2).unwrap();
    insert_memory_link(
        &conn,
        &b2.uuid.to_string(),
        &a2.uuid.to_string(),
        "supersedes",
        1.0,
    )
    .unwrap();
    conn.execute(
        "UPDATE items SET status='archived' WHERE uuid=?1",
        rusqlite::params![b2.uuid.to_string()],
    )
    .unwrap();

    let archived = archive_superseded_memories(&conn).unwrap();
    let labels: Vec<String> = archived.iter().map(|c| c.label.clone()).collect();

    let a1_label = format!("m{}", a1.display_id.unwrap_or(0));
    let a2_label = format!("m{}", a2.display_id.unwrap_or(0));
    assert!(
        labels.contains(&a1_label),
        "old with an ACTIVE superseder is archived"
    );
    assert!(
        !labels.contains(&a2_label),
        "old with an ARCHIVED superseder is preserved"
    );

    let a2_status: String = conn
        .query_row(
            "SELECT status FROM items WHERE uuid=?1",
            rusqlite::params![a2.uuid.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(a2_status, "active", "a2 remains active");
}
