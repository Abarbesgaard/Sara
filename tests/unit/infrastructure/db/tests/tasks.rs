use super::*;
use crate::infrastructure::db::*;
use crate::infrastructure::model::{Status, Task};
use chrono::Utc;

#[test]
fn set_task_files_defaults_to_manual_source() {
    let conn = mem();
    let task = seed_task(&conn);
    set_task_files(&conn, &task.uuid, &["a.rs".into(), "b.rs".into()]).unwrap();
    let sourced = get_task_files_sourced(&conn, &task.uuid).unwrap();
    assert!(sourced.iter().all(|(_, s)| s == SOURCE_MANUAL));
    assert_eq!(sourced.len(), 2);
}

#[test]
fn sourced_files_round_trip_and_split() {
    let conn = mem();
    let task = seed_task(&conn);
    set_task_files_sourced(
        &conn,
        &task.uuid,
        &[
            ("Cargo.toml".into(), SOURCE_MANUAL.into()),
            (".gitignore".into(), SOURCE_MANUAL.into()),
            ("src/llm/mod.rs".into(), SOURCE_SUGGESTED.into()),
        ],
    )
    .unwrap();

    let sourced = get_task_files_sourced(&conn, &task.uuid).unwrap();
    let manual: Vec<_> = sourced
        .iter()
        .filter(|(_, s)| s == SOURCE_MANUAL)
        .map(|(p, _)| p.clone())
        .collect();
    let suggested: Vec<_> = sourced
        .iter()
        .filter(|(_, s)| s == SOURCE_SUGGESTED)
        .map(|(p, _)| p.clone())
        .collect();
    assert_eq!(manual.len(), 2);
    assert_eq!(suggested, vec!["src/llm/mod.rs".to_string()]);
}

#[test]
fn undo_reverts_a_completed_task_to_pending() {
    let conn = mem();
    let mut task = seed_task(&conn);

    begin_undo_batch("done 1");
    task.status = Status::Completed;
    task.end = Some(Utc::now());
    task.modified = Utc::now();
    update_task(&conn, &task).unwrap();

    assert!(get_task_by_id(&conn, 1).unwrap().is_none());

    let undone = undo(&conn).unwrap();
    assert_eq!(undone.as_deref(), Some("done 1"));

    let restored = get_task_by_uuid_prefix(&conn, &task.uuid.to_string())
        .unwrap()
        .unwrap();
    assert_eq!(restored.status, Status::Pending);
    assert!(restored.end.is_none());
}

#[test]
fn undo_removes_a_newly_added_task() {
    let conn = mem();
    begin_undo_batch("add demo");
    let mut task = Task::new("demo".into(), "tk".into());
    insert_task(&conn, &mut task).unwrap();
    assert!(
        get_task_by_uuid_prefix(&conn, &task.uuid.to_string())
            .unwrap()
            .is_some()
    );

    let undone = undo(&conn).unwrap();
    assert_eq!(undone.as_deref(), Some("add demo"));
    assert!(
        get_task_by_uuid_prefix(&conn, &task.uuid.to_string())
            .unwrap()
            .is_none()
    );
}

#[test]
fn get_task_by_uuid_prefix_errors_on_ambiguous_prefix() {
    let conn = mem();

    let mut a = Task::new("task a".into(), "proj".into());
    a.uuid = uuid::Uuid::parse_str("5cb00000-0000-0000-0000-00000000000a").unwrap();
    insert_task(&conn, &mut a).unwrap();
    let mut b = Task::new("task b".into(), "proj".into());
    b.uuid = uuid::Uuid::parse_str("5cb01111-0000-0000-0000-00000000000b").unwrap();
    insert_task(&conn, &mut b).unwrap();

    assert!(
        get_task_by_uuid_prefix(&conn, "5cb0").is_err(),
        "ambiguous prefix should error instead of arbitrarily returning one task"
    );

    let only = get_task_by_uuid_prefix(&conn, "5cb00000").unwrap().unwrap();
    assert_eq!(only.uuid, a.uuid);

    assert!(
        get_task_by_uuid_prefix(&conn, "ffffffff")
            .unwrap()
            .is_none()
    );
}

#[test]
fn resolve_task_errors_on_ambiguous_uuid_prefix() {
    let conn = mem();

    let mut a = Task::new("task a".into(), "proj".into());
    a.uuid = uuid::Uuid::parse_str("ab120000-0000-0000-0000-00000000000a").unwrap();
    insert_task(&conn, &mut a).unwrap();
    let mut b = Task::new("task b".into(), "proj".into());
    b.uuid = uuid::Uuid::parse_str("ab121111-0000-0000-0000-00000000000b").unwrap();
    insert_task(&conn, &mut b).unwrap();

    assert!(resolve_task(&conn, "ab12").is_err());
}

fn task_with_uuid(conn: &Connection, uuid: &str, desc: &str) -> Task {
    let mut t = Task::new(desc.into(), "proj".into());
    t.uuid = uuid::Uuid::parse_str(uuid).unwrap();
    insert_task(conn, &mut t).unwrap();
    t
}

#[test]
fn resolve_task_stale_display_id_reports_display_id_not_uuid_ambiguity() {
    let conn = mem();
    task_with_uuid(&conn, "88cc62e9-0000-0000-0000-00000000000a", "a");
    task_with_uuid(&conn, "88a0e813-0000-0000-0000-00000000000b", "b");

    let err = resolve_task(&conn, "88").unwrap_err().to_string();
    assert!(err.contains("display id 88"), "{err}");
    assert!(!err.contains("Ambiguous"), "{err}");
}

#[test]
fn resolve_task_stale_display_id_never_matches_a_uuid_prefix() {
    let conn = mem();
    task_with_uuid(&conn, "88cc62e9-0000-0000-0000-00000000000a", "unrelated");

    assert!(resolve_task(&conn, "88").is_err());
}

#[test]
fn resolve_task_accepts_an_all_digit_uuid_prefix_of_eight_chars() {
    let conn = mem();
    let t = task_with_uuid(&conn, "12345678-0000-0000-0000-00000000000a", "digits");

    assert_eq!(resolve_task(&conn, "12345678").unwrap().uuid, t.uuid);
}

#[test]
fn ambiguous_uuid_prefix_error_lists_the_candidates() {
    let conn = mem();
    task_with_uuid(&conn, "ab120000-0000-0000-0000-00000000000a", "first task");
    task_with_uuid(&conn, "ab121111-0000-0000-0000-00000000000b", "second task");

    let err = resolve_task(&conn, "ab12").unwrap_err().to_string();
    assert!(
        err.contains("ab120000") && err.contains("first task"),
        "{err}"
    );
    assert!(
        err.contains("ab121111") && err.contains("second task"),
        "{err}"
    );
}

#[test]
fn get_task_by_uuid_prefix_treats_underscore_as_literal() {
    let conn = mem();

    let mut a = Task::new("task a".into(), "proj".into());
    a.uuid = uuid::Uuid::parse_str("ab1c0000-0000-0000-0000-00000000000a").unwrap();
    insert_task(&conn, &mut a).unwrap();

    assert!(
        get_task_by_uuid_prefix(&conn, "ab_c").unwrap().is_none(),
        "underscore in a uuid prefix must be matched literally, not as a wildcard"
    );

    assert_eq!(
        get_task_by_uuid_prefix(&conn, "ab1c")
            .unwrap()
            .unwrap()
            .uuid,
        a.uuid
    );
}

#[test]
fn find_tasks_by_file_prefix_escapes_underscore_wildcard() {
    let conn = mem();

    let mut hit = Task::new("under underscore dir".into(), "proj".into());
    insert_task(&conn, &mut hit).unwrap();
    set_task_files(&conn, &hit.uuid, &["/repo/foo_bar/x.rs".into()]).unwrap();
    let mut miss = Task::new("under collision dir".into(), "proj".into());
    insert_task(&conn, &mut miss).unwrap();
    set_task_files(&conn, &miss.uuid, &["/repo/fooXbar/y.rs".into()]).unwrap();
    conn.execute("UPDATE tasks SET status='completed'", [])
        .unwrap();

    let tasks = find_tasks_by_file(&conn, "/repo/foo_bar/", true).unwrap();
    let descs: Vec<&str> = tasks.iter().map(|t| t.description.as_str()).collect();
    assert!(descs.contains(&"under underscore dir"));
    assert!(
        !descs.contains(&"under collision dir"),
        "an underscore in the query path must not wildcard-match a sibling directory"
    );
}

#[test]
fn undo_with_empty_log_returns_none() {
    let conn = mem();
    assert!(undo(&conn).unwrap().is_none());
}

#[test]
fn undo_only_reverts_the_latest_command() {
    let conn = mem();
    let mut task = seed_task(&conn);

    begin_undo_batch("modify 1");
    task.description = "first edit".into();
    task.modified = Utc::now();
    update_task(&conn, &task).unwrap();

    begin_undo_batch("modify 1 again");
    task.description = "second edit".into();
    task.modified = Utc::now();
    update_task(&conn, &task).unwrap();

    undo(&conn).unwrap();
    let after_first_undo = get_task_by_id(&conn, 1).unwrap().unwrap();
    assert_eq!(after_first_undo.description, "first edit");

    undo(&conn).unwrap();
    let after_second_undo = get_task_by_id(&conn, 1).unwrap().unwrap();
    assert_eq!(after_second_undo.description, "demo");
}

#[test]
fn set_task_files_sourced_replaces_previous() {
    let conn = mem();
    let task = seed_task(&conn);
    set_task_files_sourced(
        &conn,
        &task.uuid,
        &[("x.rs".into(), SOURCE_SUGGESTED.into())],
    )
    .unwrap();
    set_task_files_sourced(&conn, &task.uuid, &[("y.rs".into(), SOURCE_MANUAL.into())]).unwrap();
    let sourced = get_task_files_sourced(&conn, &task.uuid).unwrap();
    assert_eq!(
        sourced,
        vec![("y.rs".to_string(), SOURCE_MANUAL.to_string())]
    );
}

#[test]
fn find_tasks_by_file_exact_returns_only_completed_tasks() {
    let conn = mem();
    let completed = make_completed_task(&conn, "fix auth", "/repo/src/auth.rs");

    let mut pending = Task::new("wip auth".to_string(), "Sara".to_string());
    insert_task(&conn, &mut pending).unwrap();
    set_task_files(&conn, &pending.uuid, &["/repo/src/auth.rs".to_string()]).unwrap();

    let hits = find_tasks_by_file(&conn, "/repo/src/auth.rs", false).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].uuid, completed.uuid);
}

#[test]
fn find_tasks_by_file_prefix_returns_all_completed_under_dir() {
    let conn = mem();
    let a = make_completed_task(&conn, "task a", "/repo/src/auth.rs");
    let b = make_completed_task(&conn, "task b", "/repo/src/model.rs");
    make_completed_task(&conn, "task c", "/repo/tests/foo.rs");

    let hits = find_tasks_by_file(&conn, "/repo/src/", true).unwrap();
    assert_eq!(hits.len(), 2);
    let uuids: Vec<_> = hits.iter().map(|t| t.uuid).collect();
    assert!(uuids.contains(&a.uuid));
    assert!(uuids.contains(&b.uuid));
}

#[test]
fn concurrent_task_inserts_get_distinct_display_ids() {
    let dir = std::env::temp_dir().join(format!("sara-race-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.db");
    {
        let mut c = Connection::open(&path).unwrap();
        set_pragmas(&c).unwrap();
        super::super::migrations::apply_migrations(&mut c).unwrap();
    }

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let mut handles = vec![];
    for n in 0..8 {
        let path = path.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            let c = Connection::open(&path).unwrap();
            set_pragmas(&c).unwrap();
            let mut t = Task::new(format!("t{n}"), "p".into());
            barrier.wait();
            insert_task(&c, &mut t).map(|_| t.id.unwrap())
        }));
    }
    let ids: Vec<i64> = handles
        .into_iter()
        .filter_map(|h| h.join().unwrap().ok())
        .collect();

    let mut uniq = ids.clone();
    uniq.sort_unstable();
    uniq.dedup();
    println!("IDS: {ids:?} inserted={} unique={}", ids.len(), uniq.len());
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        uniq.len(),
        ids.len(),
        "concurrent adds produced duplicate display ids: {ids:?}"
    );
}

#[test]
fn add_dependency_rejects_self_dependency() {
    let conn = mem();
    let a = seed_named_task(&conn, "solo");
    let err = add_dependency(&conn, &a.uuid, &a.uuid)
        .expect_err("a task must not be allowed to depend on itself");
    assert!(
        err.to_string().to_lowercase().contains("itself"),
        "error should explain the self-dependency: {err}"
    );
    assert!(
        get_blockers(&conn, &a.uuid).unwrap().is_empty(),
        "no dependency row should have been written"
    );
}

#[test]
fn add_dependency_rejects_direct_cycle() {
    let conn = mem();
    let a = seed_named_task(&conn, "a");
    let b = seed_named_task(&conn, "b");
    add_dependency(&conn, &a.uuid, &b.uuid).unwrap();

    let err = add_dependency(&conn, &b.uuid, &a.uuid)
        .expect_err("b -> a would close a two-node cycle and must be refused");
    assert!(
        err.to_string().to_lowercase().contains("cycle"),
        "error should name the cycle: {err}"
    );
    assert!(
        get_blockers(&conn, &b.uuid).unwrap().is_empty(),
        "the cycle-forming dependency must not be persisted"
    );
}

#[test]
fn add_dependency_rejects_transitive_cycle() {
    let conn = mem();
    let a = seed_named_task(&conn, "a");
    let b = seed_named_task(&conn, "b");
    let c = seed_named_task(&conn, "c");
    add_dependency(&conn, &a.uuid, &b.uuid).unwrap();
    add_dependency(&conn, &b.uuid, &c.uuid).unwrap();

    let err = add_dependency(&conn, &c.uuid, &a.uuid)
        .expect_err("c -> a closes the transitive chain a -> b -> c and must be refused");
    assert!(
        err.to_string().to_lowercase().contains("cycle"),
        "error should name the cycle: {err}"
    );
    assert!(
        get_blockers(&conn, &c.uuid).unwrap().is_empty(),
        "the cycle-forming dependency must not be persisted"
    );
}

#[test]
fn add_dependency_is_idempotent() {
    let conn = mem();
    let a = seed_named_task(&conn, "a");
    let b = seed_named_task(&conn, "b");
    add_dependency(&conn, &a.uuid, &b.uuid).unwrap();
    add_dependency(&conn, &a.uuid, &b.uuid).unwrap();

    let blockers = get_blockers(&conn, &a.uuid).unwrap();
    assert_eq!(
        blockers,
        vec![b.uuid],
        "adding the same dependency twice must not duplicate it"
    );
}

#[test]
fn remove_dependency_clears_the_blocker() {
    let conn = mem();
    let a = seed_named_task(&conn, "a");
    let b = seed_named_task(&conn, "b");
    add_dependency(&conn, &a.uuid, &b.uuid).unwrap();
    assert_eq!(get_blockers(&conn, &a.uuid).unwrap(), vec![b.uuid]);

    remove_dependency(&conn, &a.uuid, &b.uuid).unwrap();
    assert!(
        get_blockers(&conn, &a.uuid).unwrap().is_empty(),
        "removing the dependency must clear the blocker"
    );
    assert!(
        get_blocking(&conn, &b.uuid).unwrap().is_empty(),
        "the reverse (blocking) view must be cleared too"
    );
}

#[test]
fn remove_dependency_is_a_noop_when_absent() {
    let conn = mem();
    let a = seed_named_task(&conn, "a");
    let b = seed_named_task(&conn, "b");
    remove_dependency(&conn, &a.uuid, &b.uuid)
        .expect("removing a non-existent dependency should not error");
    assert!(get_blockers(&conn, &a.uuid).unwrap().is_empty());
}

#[test]
fn get_blockers_only_counts_pending_blockers() {
    let conn = mem();
    let a = seed_named_task(&conn, "a");
    let mut blocker = seed_named_task(&conn, "blocker");
    add_dependency(&conn, &a.uuid, &blocker.uuid).unwrap();
    assert_eq!(
        get_blockers(&conn, &a.uuid).unwrap(),
        vec![blocker.uuid],
        "a pending blocker is counted"
    );

    blocker.status = Status::Completed;
    update_task(&conn, &blocker).unwrap();
    assert!(
        get_blockers(&conn, &a.uuid).unwrap().is_empty(),
        "a completed blocker no longer blocks its dependent"
    );
    assert_eq!(
        get_blocking(&conn, &blocker.uuid).unwrap(),
        vec![a.uuid],
        "the dependency row still exists, so `get_blocking` still lists the dependent"
    );
}
