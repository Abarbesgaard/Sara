use super::*;

#[test]
fn add_step_stores_full_metadata_and_get_steps_filters_by_kind() {
    let conn = mem();
    let task = seed_task(&conn);
    add_step(
        &conn,
        &task.uuid,
        "wire the parser",
        Some("parse the plan JSON"),
        STEP_KIND_STEP,
        "ai",
        Some("cargo test"),
    )
    .unwrap();
    add_step(
        &conn,
        &task.uuid,
        "it compiles",
        None,
        STEP_KIND_ACCEPTANCE,
        "human",
        None,
    )
    .unwrap();

    let steps = get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap();
    assert_eq!(steps.len(), 1);
    let s = &steps[0];
    assert_eq!(s.text, "wire the parser");
    assert_eq!(s.intent.as_deref(), Some("parse the plan JSON"));
    assert_eq!(s.kind, STEP_KIND_STEP);
    assert_eq!(s.source, "ai");
    assert_eq!(s.verify_cmd.as_deref(), Some("cargo test"));
    assert!(!s.done);

    let acc = get_steps(&conn, &task.uuid, STEP_KIND_ACCEPTANCE).unwrap();
    assert_eq!(acc.len(), 1);
    assert_eq!(acc[0].text, "it compiles");
}

#[test]
fn steps_get_sequential_positions_and_index_lookup_is_one_based() {
    let conn = mem();
    let task = seed_task(&conn);
    for t in ["first", "second", "third"] {
        add_step(&conn, &task.uuid, t, None, STEP_KIND_STEP, "human", None).unwrap();
    }
    let steps = get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap();
    assert_eq!(
        steps.iter().map(|s| s.position).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );

    let id2 = step_id_by_index(&conn, &task.uuid, STEP_KIND_STEP, 2).unwrap();
    assert_eq!(id2, steps[1].id);
    assert!(step_id_by_index(&conn, &task.uuid, STEP_KIND_STEP, 99).is_err());
}

#[test]
fn move_step_reorders_within_kind_and_is_noop_at_boundaries() {
    let conn = mem();
    let task = seed_task(&conn);
    for t in ["first", "second", "third"] {
        add_step(&conn, &task.uuid, t, None, STEP_KIND_STEP, "human", None).unwrap();
    }
    add_step(
        &conn,
        &task.uuid,
        "it compiles",
        None,
        STEP_KIND_ACCEPTANCE,
        "human",
        None,
    )
    .unwrap();

    let steps = get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap();
    let second_id = steps[1].id;

    assert!(move_step(&conn, second_id, true).unwrap());
    let texts: Vec<String> = get_steps(&conn, &task.uuid, STEP_KIND_STEP)
        .unwrap()
        .into_iter()
        .map(|s| s.text)
        .collect();
    assert_eq!(texts, vec!["second", "first", "third"]);

    assert!(move_step(&conn, second_id, false).unwrap());
    let texts: Vec<String> = get_steps(&conn, &task.uuid, STEP_KIND_STEP)
        .unwrap()
        .into_iter()
        .map(|s| s.text)
        .collect();
    assert_eq!(texts, vec!["first", "second", "third"]);

    let steps = get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap();
    assert!(!move_step(&conn, steps[0].id, true).unwrap());
    assert!(!move_step(&conn, steps[2].id, false).unwrap());

    let acc = get_steps(&conn, &task.uuid, STEP_KIND_ACCEPTANCE).unwrap();
    assert_eq!(acc.len(), 1);
    assert_eq!(acc[0].text, "it compiles");
}

#[test]
fn delete_step_removes_item_and_shifts_remaining() {
    let conn = mem();
    let task = seed_task(&conn);
    for t in ["first", "second", "third"] {
        add_step(&conn, &task.uuid, t, None, STEP_KIND_STEP, "human", None).unwrap();
    }

    let mid = step_id_by_index(&conn, &task.uuid, STEP_KIND_STEP, 2).unwrap();
    delete_step(&conn, mid).unwrap();

    let steps = get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap();
    assert_eq!(
        steps.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
        vec!["first", "third"]
    );
    let now2 = step_id_by_index(&conn, &task.uuid, STEP_KIND_STEP, 2).unwrap();
    assert_eq!(now2, steps[1].id);

    let removed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_history
                 WHERE task_uuid=?1 AND field='checklist' AND new_value='removed'",
            [task.uuid.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(removed, 1);
}

#[test]
fn ensure_started_transitions_idle_task_once() {
    let conn = mem();
    let task = seed_task(&conn);
    assert!(task.started_at.is_none());

    assert!(ensure_started(&conn, &task.uuid).unwrap());
    let reloaded = get_task_by_uuid_prefix(&conn, &task.uuid.to_string())
        .unwrap()
        .unwrap();
    assert!(reloaded.started_at.is_some());

    let first_started = reloaded.started_at;
    assert!(!ensure_started(&conn, &task.uuid).unwrap());
    let again = get_task_by_uuid_prefix(&conn, &task.uuid.to_string())
        .unwrap()
        .unwrap();
    assert_eq!(again.started_at, first_started);
}

#[test]
fn set_step_done_records_result_and_commit_then_undone_clears_them() {
    let conn = mem();
    let task = seed_task(&conn);
    let id = add_step(
        &conn,
        &task.uuid,
        "do it",
        None,
        STEP_KIND_STEP,
        "human",
        None,
    )
    .unwrap();

    set_step_done(&conn, id, true, Some("all green"), Some("abc1234")).unwrap();
    let s = &get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap()[0];
    assert!(s.done);
    assert_eq!(s.result.as_deref(), Some("all green"));
    assert_eq!(s.done_commit.as_deref(), Some("abc1234"));
    assert!(s.done_at.is_some());

    set_step_done(&conn, id, false, None, None).unwrap();
    let s = &get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap()[0];
    assert!(!s.done);
    assert!(s.done_commit.is_none());
    assert!(s.done_at.is_none());
    assert_eq!(s.result.as_deref(), Some("all green"));
}
