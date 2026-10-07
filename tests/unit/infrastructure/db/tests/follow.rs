use super::*;
use chrono::Duration;

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

#[test]
fn task_activity_table_exists_on_fresh_db() {
    assert!(table_exists(&mem(), "task_activity"));
}

#[test]
fn task_activity_migration_applies_on_upgraded_db() {
    let mut conn = mem();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    conn.execute_batch("DROP TABLE task_activity;").unwrap();
    conn.pragma_update(None, "user_version", version - 1)
        .unwrap();
    super::super::migrations::apply_migrations(&mut conn).unwrap();
    assert!(table_exists(&conn, "task_activity"));

    conn.pragma_update(None, "user_version", version - 1)
        .unwrap();
    super::super::migrations::apply_migrations(&mut conn).unwrap();
}

#[test]
fn task_activity_records_text_and_client() {
    let conn = mem();
    let t = seed_task(&conn);
    let e = record_doing(
        &conn,
        &t.uuid,
        "  reading   the parser ",
        Some("copilot-cli"),
    )
    .unwrap();
    assert_eq!(e.text, "reading the parser");
    assert_eq!(e.client.as_deref(), Some("copilot-cli"));
    let latest = latest_doing(&conn, &t.uuid).unwrap().unwrap();
    assert_eq!(latest, e);
}

#[test]
fn task_activity_latest_is_the_newest_entry() {
    let conn = mem();
    let t = seed_task(&conn);
    record_doing(&conn, &t.uuid, "first", None).unwrap();
    record_doing(&conn, &t.uuid, "second", Some("")).unwrap();
    let latest = latest_doing(&conn, &t.uuid).unwrap().unwrap();
    assert_eq!(latest.text, "second");
    assert_eq!(latest.client, None);
    assert_eq!(doing_for_task(&conn, &t.uuid).unwrap().len(), 2);
}

#[test]
fn task_activity_rejects_empty_text() {
    let conn = mem();
    let t = seed_task(&conn);
    assert!(record_doing(&conn, &t.uuid, "   ", None).is_err());
    assert!(latest_doing(&conn, &t.uuid).unwrap().is_none());
}

#[test]
fn task_activity_truncates_long_text() {
    let conn = mem();
    let t = seed_task(&conn);
    let long = "x".repeat(DOING_MAX_CHARS + 50);
    let e = record_doing(&conn, &t.uuid, &long, None).unwrap();
    assert_eq!(e.text.chars().count(), DOING_MAX_CHARS);
}

#[test]
fn task_activity_keeps_a_bounded_tail_per_task() {
    let conn = mem();
    let t = seed_task(&conn);
    for i in 0..(DOING_KEEP_PER_TASK + 5) {
        record_doing(&conn, &t.uuid, &format!("step {i}"), None).unwrap();
    }
    let all = doing_for_task(&conn, &t.uuid).unwrap();
    assert_eq!(all.len() as i64, DOING_KEEP_PER_TASK);
    assert_eq!(
        all.last().unwrap().text,
        format!("step {}", DOING_KEEP_PER_TASK + 4)
    );
}

#[test]
fn task_activity_pruning_is_scoped_to_the_task() {
    let conn = mem();
    let a = seed_task(&conn);
    let b = seed_task(&conn);
    record_doing(&conn, &a.uuid, "a0", None).unwrap();
    for i in 0..(DOING_KEEP_PER_TASK + 5) {
        record_doing(&conn, &b.uuid, &format!("b{i}"), None).unwrap();
    }
    record_doing(&conn, &a.uuid, "a1", None).unwrap();
    assert_eq!(doing_for_task(&conn, &a.uuid).unwrap().len(), 2);
}

#[test]
fn task_activity_cascades_on_task_delete() {
    let conn = mem();
    let t = seed_task(&conn);
    record_doing(&conn, &t.uuid, "x", None).unwrap();
    conn.execute("DELETE FROM tasks WHERE uuid=?1", [t.uuid.to_string()])
        .unwrap();
    assert!(doing_for_task(&conn, &t.uuid).unwrap().is_empty());
}

#[test]
fn data_version_changes_when_another_connection_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sara.db");
    let mut writer = Connection::open(&path).unwrap();
    writer
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .unwrap();
    super::super::migrations::apply_migrations(&mut writer).unwrap();
    let reader = Connection::open(&path).unwrap();
    let t = seed_task(&writer);

    let before = data_version(&reader).unwrap();
    assert_eq!(data_version(&reader).unwrap(), before);
    record_doing(&writer, &t.uuid, "now", None).unwrap();
    assert_ne!(data_version(&reader).unwrap(), before);
}

#[test]
fn flow_events_union_all_sources_in_time_order() {
    let conn = mem();
    let t = seed_task(&conn);
    add_checklist_item(&conn, &t.uuid, "write parser").unwrap();
    record_doing(&conn, &t.uuid, "reading code", Some("agent")).unwrap();
    add_annotation_full(
        &conn,
        &t.uuid,
        "lexer is lossy",
        "finding",
        "agent",
        None,
        None,
        false,
    )
    .unwrap();
    let step = get_checklist(&conn, &t.uuid).unwrap().remove(0);
    set_step_done(&conn, step.id, true, Some("parser green"), None).unwrap();
    let mut m = make_memory("alpha", &[]);
    insert_item(&conn, &mut m).unwrap();
    record_memory_use(&conn, &m.uuid, &t.uuid, MemoryUseKind::Recalled).unwrap();

    let events = flow_events(&conn, &t.uuid).unwrap();
    let kinds: Vec<FlowKind> = events.iter().map(|e| e.kind.clone()).collect();
    assert!(kinds.contains(&FlowKind::StepAdded));
    assert!(kinds.contains(&FlowKind::Doing));
    assert!(kinds.contains(&FlowKind::Note("finding".into())));
    assert!(kinds.contains(&FlowKind::StepDone));
    assert!(kinds.contains(&FlowKind::Memory("recalled".into())));
    assert!(events.windows(2).all(|w| w[0].at <= w[1].at));
    let done = events
        .iter()
        .find(|e| e.kind == FlowKind::StepDone)
        .unwrap();
    assert_eq!(done.detail.as_deref(), Some("parser green"));
    assert!(
        !events
            .iter()
            .any(|e| matches!(&e.kind, FlowKind::Change(f) if f == "annotation")),
        "annotation history duplicates the note event"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.kind == FlowKind::Change("checklist".into()) && e.text.contains("step done")),
        "step-done history duplicates the step event"
    );
}

#[test]
fn flow_open_tasks_include_quiet_ones_ordered_newest_first() {
    let conn = mem();
    let quiet = seed_task(&conn);
    let older = seed_task(&conn);
    let newer = seed_task(&conn);
    conn.execute(
        "UPDATE task_history SET changed_at=?1",
        [(Utc::now() - Duration::days(3)).to_rfc3339()],
    )
    .unwrap();
    record_doing(&conn, &older.uuid, "a", None).unwrap();
    conn.execute(
        "UPDATE task_activity SET at=?1 WHERE task_uuid=?2",
        rusqlite::params![
            (Utc::now() - Duration::minutes(30)).to_rfc3339(),
            older.uuid.to_string()
        ],
    )
    .unwrap();
    record_doing(&conn, &newer.uuid, "b", None).unwrap();

    let open = open_tasks_by_activity(&conn, None).unwrap();
    let ids: Vec<_> = open.iter().map(|(t, _)| t.uuid).collect();
    assert_eq!(ids, vec![newer.uuid, older.uuid, quiet.uuid]);
}
