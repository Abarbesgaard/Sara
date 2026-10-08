use super::*;
use crate::infrastructure::model::Task;
use chrono::Utc;

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
fn reset_project_nukes_tasks_children_and_profile() {
    let mut conn = mem();
    let task = seed_task(&conn);
    set_task_files(&conn, &task.uuid, &["src/main.rs".into()]).unwrap();
    add_link(&conn, &task.uuid, "https://example.com", None).unwrap();
    add_annotation(&conn, &task.uuid, "a note").unwrap();
    save_project_profile(
        &conn,
        &crate::infrastructure::model::Project {
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
fn project_badges_count_stale_validation_and_open_feedback() {
    let conn = mem();
    let mut fresh = Task::new("fresh".into(), "demo".into());
    insert_task(&conn, &mut fresh).unwrap();
    set_validated(&conn, &fresh.uuid, "head").unwrap();
    let mut stale = Task::new("stale".into(), "demo".into());
    insert_task(&conn, &mut stale).unwrap();
    set_validated(&conn, &stale.uuid, "old").unwrap();
    let mut other = Task::new("other".into(), "else".into());
    insert_task(&conn, &mut other).unwrap();
    set_validated(&conn, &other.uuid, "old").unwrap();

    add_annotation(&conn, &fresh.uuid, "please fix").unwrap();
    let resolved = add_annotation_full(
        &conn,
        &stale.uuid,
        "done",
        NOTE_KIND_COMMENT,
        "human",
        None,
        None,
        false,
    )
    .unwrap();
    resolve_annotation(&conn, resolved, None).unwrap();
    add_annotation_full(
        &conn,
        &stale.uuid,
        "a finding",
        "finding",
        "agent",
        None,
        None,
        false,
    )
    .unwrap();

    let b = project_badges(&conn, "demo", Some("head")).unwrap();
    assert_eq!(
        b,
        ProjectBadges {
            stale: 1,
            feedback: 1
        }
    );
    assert_eq!(project_badges(&conn, "demo", None).unwrap().stale, 0);
}

#[test]
fn tasks_touched_on_matches_created_completed_and_history() {
    let conn = mem();
    let day = Utc.with_ymd_and_hms(2024, 5, 2, 12, 0, 0).unwrap();
    let mut created = Task::new("created".into(), "demo".into());
    created.entry = day;
    insert_task(&conn, &mut created).unwrap();
    let mut finished = Task::new("finished".into(), "demo".into());
    finished.entry = day - chrono::Duration::days(10);
    finished.end = Some(day);
    insert_task(&conn, &mut finished).unwrap();
    let mut untouched = Task::new("untouched".into(), "demo".into());
    untouched.entry = day - chrono::Duration::days(10);
    insert_task(&conn, &mut untouched).unwrap();
    let mut elsewhere = Task::new("elsewhere".into(), "else".into());
    elsewhere.entry = day;
    insert_task(&conn, &mut elsewhere).unwrap();

    let d = day.date_naive();
    let mut titles: Vec<String> = tasks_touched_on(&conn, d, Some("demo"))
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    titles.sort();
    assert_eq!(titles, ["created", "finished"]);
    assert_eq!(tasks_touched_on(&conn, d, None).unwrap().len(), 3);
}
