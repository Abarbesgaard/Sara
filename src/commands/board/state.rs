use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;
use crate::infrastructure::model::Status;

use super::types::{BoardState, IssueNode};

/// Load tasks for the project and group them by the GitHub issue they trace
/// back to. `prev` (the state before a reload, e.g. after returning from a
/// task's detail view) carries over each issue's expand/collapse state so
/// drilling into a task doesn't collapse the tree the user just opened.
///
/// Unless `show_finished`, completed tasks are dropped from the rows shown —
/// an issue whose tasks are all completed disappears entirely, and completed
/// standalone tasks are omitted too. Per-issue done/total counts still reflect
/// every task, so a partially-done issue's header stays accurate.
pub(super) fn build_state(
    conn: &Connection,
    project: String,
    show_finished: bool,
    prev: Option<&BoardState>,
) -> Result<BoardState> {
    let all = db::list_tasks_for_board(conn, &project)?;
    let (groups, standalone_all) = db::group_tasks_by_issue(conn, &all)?;
    let titles = db::github_issue_titles_for_project(conn, &project)?;
    let imported = db::github_synced_task_uuids(conn)?;
    let badges = db::link_flags_by_task(conn).unwrap_or_default();

    let issues: Vec<IssueNode> = groups
        .into_iter()
        .filter_map(|g| {
            let total = g.tasks.len();
            let done = g
                .tasks
                .iter()
                .filter(|t| t.status == Status::Completed)
                .count();
            let title = g
                .tasks
                .iter()
                .find_map(|t| titles.get(&t.uuid.to_string()).cloned());
            let tasks: Vec<_> = if show_finished {
                g.tasks
            } else {
                g.tasks
                    .into_iter()
                    .filter(|t| t.status != Status::Completed)
                    .collect()
            };
            if tasks.is_empty() {
                return None;
            }
            let expanded = prev
                .and_then(|p| {
                    p.issues
                        .iter()
                        .find(|i| i.owner_repo == g.owner_repo && i.number == g.number)
                })
                .map(|i| i.expanded)
                .unwrap_or(false);
            Some(IssueNode {
                owner_repo: g.owner_repo,
                number: g.number,
                title,
                tasks,
                done,
                total,
                expanded,
            })
        })
        .collect();

    let standalone: Vec<_> = if show_finished {
        standalone_all
    } else {
        standalone_all
            .into_iter()
            .filter(|t| t.status != Status::Completed)
            .collect()
    };

    let pending = all.iter().filter(|t| t.status == Status::Pending).count();
    let done = all.len() - pending;

    Ok(BoardState {
        project,
        issues,
        standalone,
        badges,
        show_finished,
        imported,
        selected: 0,
        scroll: 0,
        pending,
        done,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::model::Task;

    fn task(conn: &Connection, desc: &str) -> Task {
        let mut t = Task::new(desc.to_string(), "tk".to_string());
        db::insert_task(conn, &mut t).unwrap();
        t
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
}
