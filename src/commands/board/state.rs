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
#[path = "../../../tests/unit/commands/board/state.rs"]
mod tests;
