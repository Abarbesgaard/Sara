use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;
use crate::infrastructure::model::Status;

use crate::infrastructure::model::Task;

use super::types::{BoardState, CardInfo, Freshness, IssueNode};

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
    let head = db::get_project(conn, &project)
        .ok()
        .flatten()
        .and_then(|p| p.path)
        .and_then(|p| crate::infrastructure::git::head_commit(std::path::Path::new(&p)));
    let deps = db::dep_info_by_task(conn).unwrap_or_default();
    let cards = all
        .iter()
        .filter(|t| show_finished || t.status != Status::Completed)
        .map(|t| {
            let mut card = card_info(conn, t, head.as_deref());
            card.blocked_by = deps
                .get(&t.uuid.to_string())
                .map(|d| d.blocked_by.clone())
                .unwrap_or_default();
            (t.uuid.to_string(), card)
        })
        .collect();

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
        cards,
        filter: prev.map(|p| p.filter.clone()).unwrap_or_default(),
        filtering: false,
        preview: prev.is_some_and(|p| p.preview),
        show_finished,
        imported,
        selected: 0,
        scroll: 0,
        pending,
        done,
    })
}

pub(super) fn card_info(conn: &Connection, task: &Task, head: Option<&str>) -> CardInfo {
    let validated = db::get_guide_fields(conn, &task.uuid)
        .ok()
        .and_then(|g| g.validated_commit);
    let freshness = match (validated, head) {
        (None, _) => Freshness::Unvalidated,
        (Some(v), Some(h)) if v != h => Freshness::Stale,
        (Some(_), _) => Freshness::Valid,
    };
    let checklist = db::get_checklist(conn, &task.uuid).unwrap_or_default();
    let accept: Vec<_> = checklist
        .iter()
        .filter(|c| c.kind == db::STEP_KIND_ACCEPTANCE)
        .collect();
    let step = checklist
        .iter()
        .find(|c| c.kind == db::STEP_KIND_STEP && !c.done)
        .map(|c| c.text.clone());
    let feedback = db::get_open_feedback(conn, &task.uuid).unwrap_or_default();
    CardInfo {
        freshness,
        branch: db::get_task_branch(conn, &task.uuid).map(|b| b.branch),
        accept_done: accept.iter().filter(|c| c.done).count(),
        accept_total: accept.len(),
        step,
        feedback: feedback.len(),
        revise: feedback.iter().filter(|a| a.request_revision).count(),
        blocked_by: vec![],
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/board/state.rs"]
mod tests;
