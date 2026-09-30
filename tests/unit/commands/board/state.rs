use super::*;
use crate::infrastructure::model::Task;

fn task(conn: &Connection, desc: &str) -> Task {
    crate::test_support::seed_task(conn, desc, "tk")
}

fn complete(conn: &Connection, t: &Task) {
    let mut t = t.clone();
    t.status = Status::Completed;
    t.end = Some(chrono::Utc::now());
    t.modified = chrono::Utc::now();
    db::update_task(conn, &t).unwrap();
}

#[test]
fn groups_tasks_by_linked_issue_and_buckets_the_rest_as_standalone() {
    let conn = db::open_in_memory_for_test();
    let a = task(&conn, "a");
    let b = task(&conn, "b");
    let unlinked = task(&conn, "unlinked");
    db::add_link(&conn, &a.uuid, "https://github.com/o/r/issues/5", None).unwrap();
    db::add_link(&conn, &b.uuid, "https://github.com/o/r/issues/5", None).unwrap();

    let st = build_state(&conn, "tk".to_string(), false, None).unwrap();

    assert_eq!(st.issues.len(), 1);
    assert_eq!(st.issues[0].number, 5);
    assert_eq!(st.issues[0].tasks.len(), 2);
    assert_eq!(st.standalone.len(), 1);
    assert_eq!(st.standalone[0].uuid, unlinked.uuid);
}

#[test]
fn completed_tasks_are_hidden_unless_show_finished() {
    let conn = db::open_in_memory_for_test();
    let a = task(&conn, "a");
    let b = task(&conn, "b");
    db::add_link(&conn, &a.uuid, "https://github.com/o/r/issues/5", None).unwrap();
    db::add_link(&conn, &b.uuid, "https://github.com/o/r/issues/5", None).unwrap();
    complete(&conn, &a);

    let hidden = build_state(&conn, "tk".to_string(), false, None).unwrap();
    assert_eq!(hidden.issues[0].tasks.len(), 1);
    assert_eq!(hidden.issues[0].done, 1);
    assert_eq!(hidden.issues[0].total, 2);

    let shown = build_state(&conn, "tk".to_string(), true, None).unwrap();
    assert_eq!(shown.issues[0].tasks.len(), 2);
}

#[test]
fn issue_whose_tasks_are_all_completed_disappears_unless_show_finished() {
    let conn = db::open_in_memory_for_test();
    let a = task(&conn, "a");
    db::add_link(&conn, &a.uuid, "https://github.com/o/r/issues/5", None).unwrap();
    complete(&conn, &a);

    let hidden = build_state(&conn, "tk".to_string(), false, None).unwrap();
    assert!(hidden.issues.is_empty());

    let shown = build_state(&conn, "tk".to_string(), true, None).unwrap();
    assert_eq!(shown.issues.len(), 1);
}

#[test]
fn expand_state_carries_over_across_a_reload() {
    let conn = db::open_in_memory_for_test();
    let a = task(&conn, "a");
    db::add_link(&conn, &a.uuid, "https://github.com/o/r/issues/5", None).unwrap();

    let mut st = build_state(&conn, "tk".to_string(), false, None).unwrap();
    assert!(!st.issues[0].expanded);
    st.issues[0].expanded = true;

    let reloaded = build_state(&conn, "tk".to_string(), false, Some(&st)).unwrap();
    assert!(reloaded.issues[0].expanded);
}

#[test]
fn imported_tracks_github_synced_tasks() {
    let conn = db::open_in_memory_for_test();
    let synced = task(&conn, "synced");
    db::set_github_provenance(
        &conn,
        &synced.uuid,
        &crate::infrastructure::model::GithubProvenance {
            repo: "o/r".to_string(),
            issue_id: None,
            node_id: None,
            number: 5,
            html_url: None,
            title: Some("Some issue".to_string()),
            body: None,
            state: None,
            assignees: vec![],
            creator: None,
            updated_at: None,
            synced_at: chrono::Utc::now(),
            synced_by: None,
        },
    )
    .unwrap();
    let plain = task(&conn, "plain");

    let st = build_state(&conn, "tk".to_string(), false, None).unwrap();
    assert!(st.imported.contains(&synced.uuid.to_string()));
    assert!(!st.imported.contains(&plain.uuid.to_string()));
}
