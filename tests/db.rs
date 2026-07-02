//! Integration tests for `sara_tasks::infrastructure::db`.
//!
//! Moved out of an inline `#[cfg(test)] mod tests` block in
//! `src/infrastructure/db.rs` so it lives under `tests/` like the rest of
//! the suite. Uses the shared fixtures in `tests/common/`.

mod common;

use chrono::{TimeZone as _, Utc};
use common::{mem, seed_task};
use rusqlite::Connection;
use sara_tasks::infrastructure::db::*;
use sara_tasks::infrastructure::model::{Status, Task};
use uuid::Uuid;

fn projects_columns(conn: &Connection) -> std::collections::HashSet<String> {
    conn.prepare("PRAGMA table_info(projects)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn fresh_database_has_github_sync_columns() {
    let conn = mem();
    let cols = projects_columns(&conn);
    for col in ["github_repo", "github_login", "github_sync_scope"] {
        assert!(cols.contains(col), "fresh DB missing column {col}");
    }
}

#[test]
fn in_memory_test_db_enforces_foreign_keys() {
    // Regression for PR #58 review: open_in_memory_for_test must set the same
    // PRAGMAs as open(), notably foreign_keys=ON, so FK enforcement / cascade
    // behaviour matches production. A child row referencing a non-existent
    // task must be rejected.
    let conn = open_in_memory_for_test();
    let missing = Uuid::new_v4();
    let res = add_link(&conn, &missing, "https://example.com/pr/1", None);
    assert!(
        res.is_err(),
        "foreign_keys should be ON: linking to a non-existent task must fail"
    );
}

#[test]
fn appended_migration_backfills_github_columns_on_upgraded_db() {
    // Reproduce a database that upgraded across the point where the GitHub
    // sync columns migration was inserted mid-list: the projects table
    // predates those columns, yet user_version is already at the old list
    // length (14), so rusqlite_migration would otherwise skip the inserted
    // migration forever and the schema would stay broken.
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE projects (
            name TEXT PRIMARY KEY, path TEXT, goal TEXT, stack TEXT,
            conventions TEXT, notes TEXT, initialized_at TEXT, last_seen TEXT,
            setup_cmd TEXT, test_cmd TEXT, lint_cmd TEXT, run_cmd TEXT
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO projects (name, path) VALUES ('demo', '/tmp/demo')",
        [],
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 14_i64).unwrap();
    assert!(!projects_columns(&conn).contains("github_repo"));

    apply_migrations(&mut conn).unwrap();

    let cols = projects_columns(&conn);
    for col in ["github_repo", "github_login", "github_sync_scope"] {
        assert!(cols.contains(col), "backfill missing column {col}");
    }
    // Existing rows survive the backfill and re-running is a no-op.
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    apply_migrations(&mut conn).unwrap();
}

#[test]
fn project_last_activity_returns_latest_modified() {
    let conn = mem();
    let older = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let newer = Utc.with_ymd_and_hms(2021, 6, 1, 0, 0, 0).unwrap();

    let mut t1 = Task::new("a".into(), "demo".into());
    t1.modified = older;
    insert_task(&conn, &mut t1).unwrap();
    let mut t2 = Task::new("b".into(), "demo".into());
    t2.modified = newer;
    insert_task(&conn, &mut t2).unwrap();

    assert_eq!(project_last_activity(&conn, "demo").unwrap(), Some(newer));
    assert!(
        project_last_activity(&conn, "nonexistent")
            .unwrap()
            .is_none()
    );
}

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
fn project_names_unions_tasks_and_profiles_sorted() {
    let conn = mem();
    let mut t = Task::new("x".into(), "alpha".into());
    insert_task(&conn, &mut t).unwrap();
    upsert_project_seen(&conn, "beta", Some("/p/beta")).unwrap();

    let names = project_names(&conn).unwrap();
    assert!(names.contains(&"alpha".to_string()), "{names:?}");
    assert!(names.contains(&"beta".to_string()), "{names:?}");
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "project_names should be sorted");
}

#[test]
fn get_project_by_path_finds_registered_project() {
    let conn = mem();
    upsert_project_seen(&conn, "cardpsp-workspace", Some("/home/u/workspace")).unwrap();
    let found = get_project_by_path(&conn, "/home/u/workspace").unwrap();
    assert_eq!(found.map(|p| p.name), Some("cardpsp-workspace".to_string()));
    assert!(get_project_by_path(&conn, "/elsewhere").unwrap().is_none());
}

#[test]
fn get_project_by_path_prefers_most_recently_seen_on_collision() {
    let conn = mem();
    upsert_project_seen(&conn, "stale", Some("/home/u/workspace")).unwrap();
    upsert_project_seen(&conn, "current", Some("/home/u/workspace")).unwrap();
    // Force deterministic ordering regardless of timestamp resolution.
    conn.execute(
        "UPDATE projects SET last_seen='2020-01-01T00:00:00Z' WHERE name='stale'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE projects SET last_seen='2030-01-01T00:00:00Z' WHERE name='current'",
        [],
    )
    .unwrap();
    let found = get_project_by_path(&conn, "/home/u/workspace").unwrap();
    assert_eq!(found.map(|p| p.name), Some("current".to_string()));
}

#[test]
fn adding_annotation_records_a_history_event() {
    let conn = mem();
    let task = seed_task(&conn);
    add_annotation(&conn, &task.uuid, "This is a test comment").unwrap();

    let history = get_history(&conn, &task.uuid).unwrap();
    let ann: Vec<_> = history.iter().filter(|h| h.field == "annotation").collect();
    assert_eq!(ann.len(), 1);
    assert_eq!(ann[0].new_value.as_deref(), Some("This is a test comment"));
    assert!(ann[0].old_value.is_none());
}

#[test]
fn deleting_annotation_records_a_removal_event() {
    let conn = mem();
    let task = seed_task(&conn);
    add_annotation(&conn, &task.uuid, "temp note").unwrap();
    let anns = get_annotations(&conn, &task.uuid).unwrap();
    assert_eq!(anns.len(), 1);

    delete_annotation(&conn, anns[0].id).unwrap();

    let history = get_history(&conn, &task.uuid).unwrap();
    let removals: Vec<_> = history
        .iter()
        .filter(|h| h.field == "annotation" && h.new_value.is_none())
        .collect();
    assert_eq!(removals.len(), 1);
    assert_eq!(removals[0].old_value.as_deref(), Some("temp note"));
}

#[test]
fn reset_project_nukes_tasks_children_and_profile() {
    let mut conn = mem();
    let task = seed_task(&conn);
    // Attach children that should cascade away.
    set_task_files(&conn, &task.uuid, &["src/main.rs".into()]).unwrap();
    add_link(&conn, &task.uuid, "https://example.com", None).unwrap();
    add_annotation(&conn, &task.uuid, "a note").unwrap();
    save_project_profile(
        &conn,
        &sara_tasks::infrastructure::model::Project {
            name: "tk".into(),
            path: None,
            goal: Some("g".into()),
            stack: None,
            conventions: None,
            notes: None,
            initialized_at: None,
            last_seen: None,
            github_repo: None,
            github_login: None,
            github_sync_scope: None,
        },
    )
    .unwrap();

    assert_eq!(count_project_tasks(&conn, "tk").unwrap(), 1);
    let deleted = reset_project(&mut conn, "tk").unwrap();
    assert_eq!(deleted, 1);

    assert_eq!(count_project_tasks(&conn, "tk").unwrap(), 0);
    assert!(get_project(&conn, "tk").unwrap().is_none());
    assert!(get_task_files(&conn, &task.uuid).unwrap().is_empty());
    assert!(get_links(&conn, &task.uuid).unwrap().is_empty());
    assert!(get_annotations(&conn, &task.uuid).unwrap().is_empty());
}

#[test]
fn github_pr_url_gets_nice_label() {
    assert_eq!(
        derive_link_label("https://github.com/acme/widgets/pull/42"),
        Some("PR #42 · acme/widgets".to_string())
    );
    assert_eq!(
        derive_link_label("https://github.com/acme/widgets/issues/7"),
        Some("Issue #7 · acme/widgets".to_string())
    );
    assert_eq!(derive_link_label("https://example.com/foo"), None);
}

#[test]
fn is_issue_link_distinguishes_issues_from_prs_and_others() {
    assert!(is_issue_link("https://github.com/acme/widgets/issues/7"));
    assert!(!is_issue_link("https://github.com/acme/widgets/pull/42"));
    assert!(!is_issue_link("https://example.com/foo"));
}

#[test]
fn link_flags_by_task_distinguishes_pr_issue_and_generic_links() {
    let conn = mem();
    let pr_task = seed_task(&conn);
    let issue_task = seed_task(&conn);
    let generic_task = seed_task(&conn);

    add_link(
        &conn,
        &pr_task.uuid,
        "https://github.com/acme/widgets/pull/42",
        None,
    )
    .unwrap();
    add_link(
        &conn,
        &issue_task.uuid,
        "https://github.com/acme/widgets/issues/7",
        None,
    )
    .unwrap();
    add_link(&conn, &generic_task.uuid, "https://example.com/foo", None).unwrap();

    let flags = link_flags_by_task(&conn).unwrap();

    let pr_flags = flags[&pr_task.uuid.to_string()];
    assert!(pr_flags.any && pr_flags.pr && !pr_flags.issue);

    let issue_flags = flags[&issue_task.uuid.to_string()];
    assert!(issue_flags.any && issue_flags.issue && !issue_flags.pr);

    let generic_flags = flags[&generic_task.uuid.to_string()];
    assert!(generic_flags.any && !generic_flags.pr && !generic_flags.issue);
}

#[test]
fn parse_issue_link_extracts_owner_repo_and_number() {
    assert_eq!(
        parse_issue_link("https://github.com/acme/widgets/issues/7"),
        Some(("acme/widgets".to_string(), 7))
    );
    assert_eq!(
        parse_issue_link("https://github.com/acme/widgets/pull/42"),
        None
    );
    assert_eq!(parse_issue_link("https://example.com/foo"), None);
}

#[test]
fn group_tasks_by_issue_groups_shared_issues_and_buckets_the_rest() {
    let conn = mem();
    let t1 = seed_task(&conn);
    let t2 = seed_task(&conn);
    let t3 = seed_task(&conn);
    let unlinked = seed_task(&conn);

    // t1 and t2 both trace back to the same issue; t3 to a different one.
    add_link(
        &conn,
        &t1.uuid,
        "https://github.com/acme/widgets/issues/7",
        None,
    )
    .unwrap();
    add_link(
        &conn,
        &t2.uuid,
        "https://github.com/acme/widgets/issues/7",
        None,
    )
    .unwrap();
    add_link(
        &conn,
        &t3.uuid,
        "https://github.com/acme/widgets/issues/9",
        None,
    )
    .unwrap();

    let tasks = vec![t1.clone(), t2.clone(), t3.clone(), unlinked.clone()];
    let (groups, ungrouped) = group_tasks_by_issue(&conn, &tasks).unwrap();

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].owner_repo, "acme/widgets");
    assert_eq!(groups[0].number, 7);
    assert_eq!(
        groups[0].tasks.iter().map(|t| t.uuid).collect::<Vec<_>>(),
        vec![t1.uuid, t2.uuid]
    );
    assert_eq!(groups[1].number, 9);
    assert_eq!(
        groups[1].tasks.iter().map(|t| t.uuid).collect::<Vec<_>>(),
        vec![t3.uuid]
    );

    assert_eq!(ungrouped.len(), 1);
    assert_eq!(ungrouped[0].uuid, unlinked.uuid);
}

#[test]
fn is_url_detects_links_vs_paths() {
    assert!(is_url("https://github.com/a/b/pull/1"));
    assert!(is_url("http://example.com"));
    assert!(is_url("www.test.dk"));
    assert!(!is_url("src/main.rs"));
    assert!(!is_url("Cargo.toml"));
}

#[test]
fn add_and_get_links_with_history() {
    let conn = mem();
    let task = seed_task(&conn);
    add_link(
        &conn,
        &task.uuid,
        "https://github.com/acme/widgets/pull/42",
        None,
    )
    .unwrap();
    let links = get_links(&conn, &task.uuid).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].display(), "PR #42 · acme/widgets");

    // History event recorded for the added link.
    let history = get_history(&conn, &task.uuid).unwrap();
    assert!(
        history
            .iter()
            .any(|h| h.field == "link" && h.new_value.as_deref() == Some("PR #42 · acme/widgets"))
    );
}

#[test]
fn delete_link_records_removal_history() {
    let conn = mem();
    let task = seed_task(&conn);
    add_link(&conn, &task.uuid, "https://example.com/x", Some("My link")).unwrap();
    let links = get_links(&conn, &task.uuid).unwrap();
    assert!(delete_link(&conn, links[0].id).unwrap());
    assert!(get_links(&conn, &task.uuid).unwrap().is_empty());

    let history = get_history(&conn, &task.uuid).unwrap();
    assert!(
        history
            .iter()
            .any(|h| h.field == "link" && h.old_value.as_deref() == Some("My link"))
    );
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

    // Task is now completed and no longer pending.
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

fn seed_named_task(conn: &Connection, desc: &str) -> Task {
    let mut task = Task::new(desc.into(), "demo".into());
    insert_task(conn, &mut task).unwrap();
    task
}

// ── steps / acceptance criteria ─────────────────────────────────────────

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
    // An acceptance row must stay put while steps are reordered.
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

    // Move "second" up -> order becomes second, first, third.
    assert!(move_step(&conn, second_id, true).unwrap());
    let texts: Vec<String> = get_steps(&conn, &task.uuid, STEP_KIND_STEP)
        .unwrap()
        .into_iter()
        .map(|s| s.text)
        .collect();
    assert_eq!(texts, vec!["second", "first", "third"]);

    // Move it down -> back to first, second, third.
    assert!(move_step(&conn, second_id, false).unwrap());
    let texts: Vec<String> = get_steps(&conn, &task.uuid, STEP_KIND_STEP)
        .unwrap()
        .into_iter()
        .map(|s| s.text)
        .collect();
    assert_eq!(texts, vec!["first", "second", "third"]);

    // Top item up and bottom item down are no-ops.
    let steps = get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap();
    assert!(!move_step(&conn, steps[0].id, true).unwrap());
    assert!(!move_step(&conn, steps[2].id, false).unwrap());

    // The acceptance row was never touched.
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

    // Remove the middle item by its stable id.
    let mid = step_id_by_index(&conn, &task.uuid, STEP_KIND_STEP, 2).unwrap();
    delete_step(&conn, mid).unwrap();

    let steps = get_steps(&conn, &task.uuid, STEP_KIND_STEP).unwrap();
    assert_eq!(
        steps.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
        vec!["first", "third"]
    );
    // The former #3 ("third") is now reachable at #2.
    let now2 = step_id_by_index(&conn, &task.uuid, STEP_KIND_STEP, 2).unwrap();
    assert_eq!(now2, steps[1].id);

    // Removal is recorded in task history.
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
    // Result is preserved across reopen (COALESCE only writes, never clears it).
    assert_eq!(s.result.as_deref(), Some("all green"));
}

// ── code anchors ────────────────────────────────────────────────────────

#[test]
fn add_task_file_upserts_anchor_metadata() {
    let conn = mem();
    let task = seed_task(&conn);
    add_task_file(
        &conn,
        &task.uuid,
        "src/db.rs",
        SOURCE_SUGGESTED,
        Some("initial reason"),
        None,
        None,
        None,
    )
    .unwrap();
    // Same path again: ON CONFLICT updates in place, no duplicate row.
    add_task_file(
        &conn,
        &task.uuid,
        "src/db.rs",
        SOURCE_MANUAL,
        Some("better reason"),
        Some("add_step"),
        Some(10),
        Some(57),
    )
    .unwrap();

    let anchors = get_task_anchors(&conn, &task.uuid).unwrap();
    assert_eq!(anchors.len(), 1);
    let a = &anchors[0];
    assert_eq!(a.source, SOURCE_MANUAL);
    assert_eq!(a.reason.as_deref(), Some("better reason"));
    assert_eq!(a.symbol.as_deref(), Some("add_step"));
    assert_eq!((a.line_start, a.line_end), (Some(10), Some(57)));
    assert_eq!(a.location(), " :: add_step (10-57)");
}

#[test]
fn anchor_location_formats_partial_ranges() {
    let single = Anchor {
        path: "x".into(),
        source: SOURCE_MANUAL.into(),
        reason: None,
        symbol: None,
        line_start: Some(42),
        line_end: None,
    };
    assert_eq!(single.location(), " (L42)");

    let bare = Anchor {
        path: "x".into(),
        source: SOURCE_MANUAL.into(),
        reason: None,
        symbol: Some("foo".into()),
        line_start: None,
        line_end: None,
    };
    assert_eq!(bare.location(), " :: foo");
}

// ── guide fields ────────────────────────────────────────────────────────

#[test]
fn guide_fields_round_trip() {
    let conn = mem();
    let task = seed_task(&conn);
    set_assignment(&conn, &task.uuid, "the original prompt").unwrap();
    set_rationale(&conn, &task.uuid, "because reasons").unwrap();
    set_validated(&conn, &task.uuid, "deadbeef").unwrap();
    set_meta_json(&conn, &task.uuid, r#"{"k":1}"#).unwrap();

    let g = get_guide_fields(&conn, &task.uuid).unwrap();
    assert_eq!(g.assignment.as_deref(), Some("the original prompt"));
    assert_eq!(g.rationale.as_deref(), Some("because reasons"));
    assert_eq!(g.validated_commit.as_deref(), Some("deadbeef"));
    assert!(g.validated_at.is_some());
    assert_eq!(g.meta_json.as_deref(), Some(r#"{"k":1}"#));
}

// ── AI run audit trail ──────────────────────────────────────────────────

#[test]
fn ai_runs_are_recorded_and_returned_in_order() {
    let conn = mem();
    let task = seed_task(&conn);
    let r1 = record_ai_run(
        &conn,
        &task.uuid,
        "enrich",
        Some("opus"),
        Some("azure"),
        Some("prompt"),
        Some("{}"),
    )
    .unwrap();
    let r2 = record_ai_run(&conn, &task.uuid, "refine", None, None, None, None).unwrap();
    assert!(r2 > r1);

    let runs = get_ai_runs(&conn, &task.uuid).unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].kind, "enrich");
    assert_eq!(runs[0].model.as_deref(), Some("opus"));
    assert_eq!(runs[1].kind, "refine");
    assert!(runs[1].model.is_none());
}

// ── feedback lifecycle ──────────────────────────────────────────────────

#[test]
fn open_feedback_lists_comments_flagged_first_and_resolves() {
    let conn = mem();
    let task = seed_task(&conn);
    // A plain comment, a flagged comment, and a non-comment note.
    add_annotation_full(
        &conn, &task.uuid, "plain", "comment", "human", None, None, false,
    )
    .unwrap();
    let flagged = add_annotation_full(
        &conn,
        &task.uuid,
        "reconsider this",
        "comment",
        "human",
        Some("step"),
        Some("2"),
        true,
    )
    .unwrap();
    add_annotation_full(
        &conn,
        &task.uuid,
        "a finding",
        "finding",
        "ai",
        None,
        None,
        false,
    )
    .unwrap();

    let open = get_open_feedback(&conn, &task.uuid).unwrap();
    assert_eq!(open.len(), 2, "only open comments count as feedback");
    assert_eq!(
        open[0].text, "reconsider this",
        "flagged feedback sorts first"
    );

    // Resolving links the run and drops it from the open set.
    assert!(resolve_annotation(&conn, flagged, Some(7)).unwrap());
    let open = get_open_feedback(&conn, &task.uuid).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].text, "plain");
}

// ── cross-task FTS memory ───────────────────────────────────────────────

#[test]
fn search_fts_matches_tasks_notes_and_anchors() {
    let conn = mem();
    let task = seed_named_task(&conn, "implement frobnicator widget");
    add_annotation_full(
        &conn,
        &task.uuid,
        "the frobnicator caches results",
        "finding",
        "ai",
        None,
        None,
        false,
    )
    .unwrap();
    add_task_file(
        &conn,
        &task.uuid,
        "src/frob.rs",
        SOURCE_MANUAL,
        Some("frobnicator lives here"),
        None,
        None,
        None,
    )
    .unwrap();

    let kinds: std::collections::HashSet<String> = search_fts(&conn, "frobnicator", 50)
        .unwrap()
        .into_iter()
        .map(|h| h.ref_kind)
        .collect();
    assert!(kinds.contains("task"));
    assert!(kinds.contains("note"));
    assert!(kinds.contains("anchor"));
}

#[test]
fn search_fts_tolerates_quotes_in_query() {
    let conn = mem();
    let task = seed_named_task(&conn, "handle the \"weird\" input");
    // A query containing a double-quote must not blow up the FTS parser.
    let hits = search_fts(&conn, "\"weird\" input", 10).unwrap();
    assert!(hits.iter().any(|h| h.task_uuid == task.uuid.to_string()));
}

// ── dependency closure ──────────────────────────────────────────────────

#[test]
fn dependency_closure_returns_blockers_first() {
    let conn = mem();
    // c depends on b, b depends on a  →  closure of c is [a, b, c].
    let a = seed_named_task(&conn, "a");
    let b = seed_named_task(&conn, "b");
    let c = seed_named_task(&conn, "c");
    add_dependency(&conn, &b.uuid, &a.uuid).unwrap();
    add_dependency(&conn, &c.uuid, &b.uuid).unwrap();

    let closure = dependency_closure(&conn, &c.uuid).unwrap();
    assert_eq!(closure, vec![a.uuid, b.uuid, c.uuid]);
}

// ── feature chain (board / detail panel) ────────────────────────────────

#[test]
fn feature_chain_returns_linked_tasks_blockers_first() {
    let conn = mem();
    // c → b → a (c depends on b depends on a). Queried from the middle (b),
    // the whole chain comes back in blockers-first order.
    let a = seed_named_task(&conn, "a");
    let b = seed_named_task(&conn, "b");
    let c = seed_named_task(&conn, "c");
    add_dependency(&conn, &b.uuid, &a.uuid).unwrap();
    add_dependency(&conn, &c.uuid, &b.uuid).unwrap();

    let chain: Vec<Uuid> = feature_chain(&conn, &b.uuid)
        .unwrap()
        .into_iter()
        .map(|t| t.uuid)
        .collect();
    assert_eq!(chain, vec![a.uuid, b.uuid, c.uuid]);
}

#[test]
fn feature_chain_is_empty_for_standalone_task() {
    let conn = mem();
    let a = seed_named_task(&conn, "a");
    // No dependencies → no chain to draw.
    assert!(feature_chain(&conn, &a.uuid).unwrap().is_empty());
}

#[test]
fn feature_chain_excludes_unrelated_tasks() {
    let conn = mem();
    let a = seed_named_task(&conn, "a");
    let b = seed_named_task(&conn, "b");
    let unrelated = seed_named_task(&conn, "unrelated");
    add_dependency(&conn, &b.uuid, &a.uuid).unwrap();

    let chain: Vec<Uuid> = feature_chain(&conn, &a.uuid)
        .unwrap()
        .into_iter()
        .map(|t| t.uuid)
        .collect();
    assert_eq!(chain, vec![a.uuid, b.uuid]);
    assert!(!chain.contains(&unrelated.uuid));
}

// ── project commands ────────────────────────────────────────────────────

#[test]
fn project_commands_round_trip_and_partial_update_preserves_others() {
    let conn = mem();
    set_project_commands(
        &conn,
        "demo",
        &ProjectCommands {
            setup_cmd: Some("cargo fetch".into()),
            test_cmd: Some("cargo test".into()),
            lint_cmd: None,
            run_cmd: None,
        },
    )
    .unwrap();
    // A partial update (only lint) must COALESCE-preserve the earlier commands.
    set_project_commands(
        &conn,
        "demo",
        &ProjectCommands {
            setup_cmd: None,
            test_cmd: None,
            lint_cmd: Some("cargo clippy".into()),
            run_cmd: None,
        },
    )
    .unwrap();

    let c = get_project_commands(&conn, "demo").unwrap();
    assert_eq!(c.setup_cmd.as_deref(), Some("cargo fetch"));
    assert_eq!(c.test_cmd.as_deref(), Some("cargo test"));
    assert_eq!(c.lint_cmd.as_deref(), Some("cargo clippy"));
    assert!(c.run_cmd.is_none());
}

#[test]
fn get_project_commands_defaults_to_empty_when_absent() {
    let conn = mem();
    let c = get_project_commands(&conn, "nope").unwrap();
    assert!(c.setup_cmd.is_none() && c.test_cmd.is_none());
}

// ── GitHub sync settings ─────────────────────────────────────────────────

#[test]
fn github_sync_settings_round_trip_through_project_storage() {
    let conn = mem();
    upsert_project_seen(&conn, "myrepo", Some("/home/u/myrepo")).unwrap();
    set_github_sync(
        &conn,
        "myrepo",
        &GithubSyncSettings {
            repo: Some("acme/myrepo".into()),
            login: Some("alice".into()),
            scope: Some("issues".into()),
        },
    )
    .unwrap();

    let s = get_github_sync(&conn, "myrepo").unwrap();
    assert_eq!(s.repo.as_deref(), Some("acme/myrepo"));
    assert_eq!(s.login.as_deref(), Some("alice"));
    assert_eq!(s.scope.as_deref(), Some("issues"));
}

#[test]
fn save_project_profile_persists_github_fields() {
    let conn = mem();
    save_project_profile(
        &conn,
        &sara_tasks::infrastructure::model::Project {
            name: "myrepo".into(),
            path: Some("/home/u/myrepo".into()),
            goal: Some("g".into()),
            stack: None,
            conventions: None,
            notes: None,
            initialized_at: None,
            last_seen: None,
            github_repo: Some("acme/myrepo".into()),
            github_login: Some("alice".into()),
            github_sync_scope: Some("issues".into()),
        },
    )
    .unwrap();

    let project = get_project(&conn, "myrepo").unwrap().unwrap();
    assert_eq!(project.github_repo.as_deref(), Some("acme/myrepo"));
    assert_eq!(project.github_login.as_deref(), Some("alice"));
    assert_eq!(project.github_sync_scope.as_deref(), Some("issues"));
}

#[test]
fn github_sync_partial_update_preserves_existing_fields() {
    let conn = mem();
    upsert_project_seen(&conn, "p", None).unwrap();
    set_github_sync(
        &conn,
        "p",
        &GithubSyncSettings {
            repo: Some("org/p".into()),
            login: Some("bob".into()),
            scope: Some("issues".into()),
        },
    )
    .unwrap();
    // Update only scope — repo and login must be preserved (COALESCE).
    set_github_sync(
        &conn,
        "p",
        &GithubSyncSettings {
            repo: None,
            login: None,
            scope: Some("issues,prs".into()),
        },
    )
    .unwrap();

    let s = get_github_sync(&conn, "p").unwrap();
    assert_eq!(s.repo.as_deref(), Some("org/p"), "repo preserved");
    assert_eq!(s.login.as_deref(), Some("bob"), "login preserved");
    assert_eq!(s.scope.as_deref(), Some("issues,prs"), "scope updated");
}

#[test]
fn github_sync_no_secret_field_in_settings_struct() {
    // login is a username, not a token — the type only accepts non-secret strings.
    let s = GithubSyncSettings {
        repo: Some("org/repo".into()),
        login: Some("user".into()),
        scope: Some("issues".into()),
    };
    assert!(
        !s.login.as_deref().unwrap_or("").starts_with("ghp_"),
        "login field should hold a username, not a PAT"
    );
}

#[test]
fn project_detection_loads_github_sync_metadata_for_path() {
    let conn = mem();
    upsert_project_seen(&conn, "sara", Some("/home/u/Sara")).unwrap();
    set_github_sync(
        &conn,
        "sara",
        &GithubSyncSettings {
            repo: Some("acme/sara".into()),
            login: Some("alice".into()),
            scope: Some("issues".into()),
        },
    )
    .unwrap();

    // Simulates what detect_current_project returns: the project loaded by path.
    let project = get_project_by_path(&conn, "/home/u/Sara")
        .unwrap()
        .expect("project must be found by path");

    assert_eq!(project.name, "sara");
    assert_eq!(project.github_repo.as_deref(), Some("acme/sara"));
    assert_eq!(project.github_login.as_deref(), Some("alice"));
    assert_eq!(project.github_sync_scope.as_deref(), Some("issues"));
}

// ── GitHub issue provenance ──────────────────────────────────────────────

#[test]
fn github_provenance_round_trips_through_meta_json() {
    let conn = mem();
    let task = seed_task(&conn);

    let prov = sara_tasks::infrastructure::model::GithubProvenance {
        repo: "acme/widgets".into(),
        issue_id: Some(42),
        node_id: Some("NODE42".into()),
        number: 99,
        html_url: Some("https://github.com/acme/widgets/issues/99".into()),
        title: Some("Fix widget".into()),
        body: Some("body".into()),
        state: Some("open".into()),
        assignees: vec!["alice".into()],
        creator: Some("alice".into()),
        updated_at: Some(Utc::now()),
        synced_at: Utc::now(),
        synced_by: Some("alice".into()),
    };
    set_github_provenance(&conn, &task.uuid, &prov).unwrap();

    let loaded = get_github_provenance(&conn, &task.uuid)
        .unwrap()
        .expect("provenance must be present");
    assert_eq!(loaded.repo, "acme/widgets");
    assert_eq!(loaded.number, 99);
    assert_eq!(loaded.synced_by.as_deref(), Some("alice"));
    assert_eq!(loaded.issue_id, Some(42));
    assert_eq!(loaded.node_id.as_deref(), Some("NODE42"));
}

#[test]
fn github_provenance_merges_with_existing_meta_json_keys() {
    let conn = mem();
    let task = seed_task(&conn);

    // Pre-populate meta_json with some other data.
    set_meta_json(&conn, &task.uuid, r#"{"my_key":"keep_me"}"#).unwrap();

    let prov = sara_tasks::infrastructure::model::GithubProvenance {
        repo: "org/repo".into(),
        issue_id: None,
        node_id: None,
        number: 1,
        html_url: Some("https://github.com/org/repo/issues/1".into()),
        title: Some("Issue".into()),
        body: None,
        state: Some("open".into()),
        assignees: vec![],
        creator: Some("alice".into()),
        updated_at: Some(Utc::now()),
        synced_at: Utc::now(),
        synced_by: None,
    };
    set_github_provenance(&conn, &task.uuid, &prov).unwrap();

    let raw = get_guide_fields(&conn, &task.uuid)
        .unwrap()
        .meta_json
        .unwrap();
    let obj: serde_json::Value = serde_json::from_str(&raw).unwrap();
    // Both the existing key and the new github key must be present.
    assert_eq!(obj["my_key"], "keep_me");
    assert_eq!(obj["github"]["repo"], "org/repo");
    assert_eq!(obj["github"]["number"], 1);
}

#[test]
fn github_provenance_contains_no_secret_fields() {
    // GithubProvenance only stores remote identity and sync metadata.
    let prov = sara_tasks::infrastructure::model::GithubProvenance {
        repo: "org/repo".into(),
        issue_id: Some(7),
        node_id: Some("NODE7".into()),
        number: 5,
        html_url: Some("https://github.com/org/repo/issues/5".into()),
        title: Some("Issue".into()),
        body: Some("body".into()),
        state: Some("open".into()),
        assignees: vec!["bob".into()],
        creator: Some("bob".into()),
        updated_at: Some(Utc::now()),
        synced_at: Utc::now(),
        synced_by: Some("bob".into()),
    };
    let serialized = serde_json::to_string(&prov).unwrap();
    // Sanity: no "token" or "pat" key appears in the serialised provenance.
    assert!(!serialized.to_lowercase().contains("token"));
    assert!(!serialized.to_lowercase().contains(r#""pat""#));
}

#[test]
fn find_github_task_uuid_matches_repo_and_number_or_node_id() {
    let conn = mem();
    let task = seed_task(&conn);
    let prov = sara_tasks::infrastructure::model::GithubProvenance {
        repo: "acme/widgets".into(),
        issue_id: Some(100),
        node_id: Some("NODE100".into()),
        number: 8,
        html_url: Some("https://github.com/acme/widgets/issues/8".into()),
        title: Some("Issue".into()),
        body: None,
        state: Some("open".into()),
        assignees: vec![],
        creator: Some("alice".into()),
        updated_at: Some(Utc::now()),
        synced_at: Utc::now(),
        synced_by: Some("alice".into()),
    };
    set_github_provenance(&conn, &task.uuid, &prov).unwrap();

    let by_number = find_github_task_uuid(&conn, "acme/widgets", 8, None)
        .unwrap()
        .expect("match by number");
    assert_eq!(by_number, task.uuid);

    let by_node = find_github_task_uuid(&conn, "acme/widgets", 999, Some("NODE100"))
        .unwrap()
        .expect("match by node id");
    assert_eq!(by_node, task.uuid);
}

// ── GitHub issue comments ────────────────────────────────────────────────

fn make_gh_comment(
    id: i64,
    author: &str,
    body: &str,
) -> sara_tasks::infrastructure::model::GithubComment {
    sara_tasks::infrastructure::model::GithubComment {
        comment_id: id,
        author: author.to_string(),
        body: body.to_string(),
        url: format!("https://github.com/a/b/issues/1#issuecomment-{id}"),
        created_at: Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap(),
        updated_at: Utc.with_ymd_and_hms(2026, 6, 2, 0, 0, 0).unwrap(),
    }
}

#[test]
fn github_comment_annotation_is_inserted_once() {
    let conn = mem();
    let task = seed_task(&conn);
    let c = make_gh_comment(42, "alice", "Looks good");

    let first = upsert_github_comment_annotation(&conn, &task.uuid, &c).unwrap();
    assert!(first, "first insert should return true");

    let second = upsert_github_comment_annotation(&conn, &task.uuid, &c).unwrap();
    assert!(!second, "duplicate insert should return false");

    let anns = get_annotations(&conn, &task.uuid).unwrap();
    assert_eq!(anns.len(), 1, "only one annotation must exist");
    assert_eq!(anns[0].author, "alice");
    assert_eq!(anns[0].text, "Looks good");
    assert_eq!(
        anns[0].target_kind.as_deref(),
        Some(NOTE_KIND_GITHUB_COMMENT)
    );
    assert_eq!(anns[0].target_id.as_deref(), Some("42"));
}

#[test]
fn github_comment_kind_is_comment_for_info_visibility() {
    // Comments must use kind="comment" so sara info shows them.
    let conn = mem();
    let task = seed_task(&conn);
    let c = make_gh_comment(7, "bob", "Fix it");
    upsert_github_comment_annotation(&conn, &task.uuid, &c).unwrap();

    let anns = get_annotations(&conn, &task.uuid).unwrap();
    assert_eq!(anns[0].kind, "comment");
}

#[test]
fn github_comment_annotation_uses_github_created_at_as_entry() {
    use chrono::TimeZone;
    let conn = mem();
    let task = seed_task(&conn);
    let created = Utc.with_ymd_and_hms(2025, 3, 15, 8, 0, 0).unwrap();
    let mut c = make_gh_comment(10, "carol", "hello");
    c.created_at = created;
    upsert_github_comment_annotation(&conn, &task.uuid, &c).unwrap();

    let anns = get_annotations(&conn, &task.uuid).unwrap();
    assert_eq!(
        anns[0].entry.timestamp(),
        created.timestamp(),
        "entry must equal the comment's created_at"
    );
}

#[test]
fn github_comments_round_trip_through_meta_json() {
    let conn = mem();
    let task = seed_task(&conn);

    let comments = vec![
        make_gh_comment(1, "alice", "First comment"),
        make_gh_comment(2, "bob", "Second comment"),
    ];
    set_github_comments(&conn, &task.uuid, &comments).unwrap();

    let loaded = get_github_comments(&conn, &task.uuid).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].comment_id, 1);
    assert_eq!(loaded[0].author, "alice");
    assert_eq!(loaded[0].body, "First comment");
    assert_eq!(
        loaded[0].url,
        "https://github.com/a/b/issues/1#issuecomment-1"
    );
    assert_eq!(loaded[1].comment_id, 2);
}

#[test]
fn github_comments_meta_json_preserves_other_keys() {
    let conn = mem();
    let task = seed_task(&conn);

    set_meta_json(&conn, &task.uuid, r#"{"other_key":"keep_me"}"#).unwrap();
    set_github_comments(&conn, &task.uuid, &[make_gh_comment(5, "dave", "hi")]).unwrap();

    let raw = get_guide_fields(&conn, &task.uuid)
        .unwrap()
        .meta_json
        .unwrap();
    let obj: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        obj["other_key"], "keep_me",
        "existing keys must be preserved"
    );
    assert_eq!(obj["github_comments"][0]["comment_id"], 5);
}

#[test]
fn repeated_set_github_comments_replaces_array() {
    let conn = mem();
    let task = seed_task(&conn);

    set_github_comments(&conn, &task.uuid, &[make_gh_comment(1, "a", "old")]).unwrap();
    set_github_comments(
        &conn,
        &task.uuid,
        &[
            make_gh_comment(1, "a", "old"),
            make_gh_comment(2, "b", "new"),
        ],
    )
    .unwrap();

    let loaded = get_github_comments(&conn, &task.uuid).unwrap();
    assert_eq!(loaded.len(), 2, "array is replaced with the latest set");
}

#[test]
fn upsert_github_comment_idempotent_across_multiple_calls() {
    // Simulates two consecutive sync runs with the same comment list.
    let conn = mem();
    let task = seed_task(&conn);
    let comments = vec![
        make_gh_comment(100, "alice", "LGTM"),
        make_gh_comment(101, "bob", "Please clarify"),
    ];

    // First sync
    for c in &comments {
        upsert_github_comment_annotation(&conn, &task.uuid, c).unwrap();
    }
    set_github_comments(&conn, &task.uuid, &comments).unwrap();

    // Second sync (same data)
    for c in &comments {
        let inserted = upsert_github_comment_annotation(&conn, &task.uuid, c).unwrap();
        assert!(
            !inserted,
            "second sync must not re-insert comment {}",
            c.comment_id
        );
    }
    set_github_comments(&conn, &task.uuid, &comments).unwrap();

    // Exactly two annotations, no duplicates.
    let anns = get_annotations(&conn, &task.uuid).unwrap();
    assert_eq!(
        anns.len(),
        2,
        "no duplicate annotations after repeated sync"
    );

    // Meta JSON also holds exactly two entries.
    let meta = get_github_comments(&conn, &task.uuid).unwrap();
    assert_eq!(meta.len(), 2);
}

#[test]
fn github_comment_metadata_preserves_url_and_updated_at() {
    let conn = mem();
    let task = seed_task(&conn);
    let mut c = make_gh_comment(77, "eve", "test");
    c.url = "https://github.com/org/repo/issues/3#issuecomment-77".to_string();
    c.updated_at = Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();

    set_github_comments(&conn, &task.uuid, &[c.clone()]).unwrap();
    let loaded = get_github_comments(&conn, &task.uuid).unwrap();

    assert_eq!(
        loaded[0].url,
        "https://github.com/org/repo/issues/3#issuecomment-77"
    );
    assert_eq!(loaded[0].updated_at, c.updated_at);
}
