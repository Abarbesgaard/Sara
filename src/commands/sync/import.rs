use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Task;

use super::github::{GhComment, GhIssue};

pub(super) fn issue_provenance(
    repo: &str,
    sync_login: &str,
    issue: &GhIssue,
) -> crate::infrastructure::model::GithubProvenance {
    crate::infrastructure::model::GithubProvenance {
        repo: repo.to_string(),
        issue_id: Some(issue.id),
        node_id: issue.node_id.clone(),
        number: issue.number,
        html_url: Some(issue.html_url.clone()),
        title: Some(issue.title.clone()),
        body: issue.body.clone(),
        state: Some(issue.state.clone()),
        assignees: issue.assignees.iter().map(|a| a.login.clone()).collect(),
        creator: Some(issue.user.login.clone()),
        updated_at: Some(issue.updated_at),
        synced_at: Utc::now(),
        synced_by: Some(sync_login.to_string()),
    }
}

pub(super) fn ensure_issue_link(
    conn: &Connection,
    task_uuid: &uuid::Uuid,
    issue: &GhIssue,
) -> Result<()> {
    let links = db::get_links(conn, task_uuid)?;
    if links.iter().any(|link| link.url == issue.html_url) {
        return Ok(());
    }
    db::add_link(
        conn,
        task_uuid,
        &issue.html_url,
        Some(&format!("#{}", issue.number)),
    )
}

pub(super) fn update_existing_task(
    conn: &Connection,
    cfg: &Config,
    task_uuid: &uuid::Uuid,
    issue: &GhIssue,
    repo: &str,
    login: &str,
) -> Result<()> {
    let mut task = db::get_task_by_uuid_prefix(conn, &task_uuid.to_string())?
        .ok_or_else(|| anyhow::anyhow!("missing imported task {task_uuid}"))?;
    task.description = issue.title.clone();
    task.modified = Utc::now();
    if !task.tags.iter().any(|t| t == "github") {
        task.tags.push("github".to_string());
    }
    task.urgency = db::compute_urgency(&task, &cfg.urgency, false, 0);
    db::update_task(conn, &task)?;

    if let Some(body) = issue.body.as_deref() {
        let body = body.trim();
        if !body.is_empty() {
            db::set_assignment(conn, &task.uuid, body)?;
        }
    }

    db::set_github_provenance(conn, &task.uuid, &issue_provenance(repo, login, issue))?;
    ensure_issue_link(conn, &task.uuid, issue)?;
    Ok(())
}

pub(super) fn create_new_task(
    conn: &Connection,
    cfg: &Config,
    project_name: &str,
    repo: &str,
    login: &str,
    issue: &GhIssue,
) -> Result<uuid::Uuid> {
    let mut task = Task::new(issue.title.clone(), project_name.to_string());
    task.tags.push("github".to_string());
    task.urgency = db::compute_urgency(&task, &cfg.urgency, false, 0);
    db::insert_task(conn, &mut task)?;
    db::set_github_provenance(conn, &task.uuid, &issue_provenance(repo, login, issue))?;
    ensure_issue_link(conn, &task.uuid, issue)?;

    if let Some(body) = issue.body.as_deref() {
        let body = body.trim();
        if !body.is_empty() {
            db::set_assignment(conn, &task.uuid, body)?;
        }
    }

    Ok(task.uuid)
}

/// Import comments for a single issue into the task's annotation list.
///
/// Each comment is stored idempotently: the stable `comment_id` is used as the
/// deduplication key so repeated syncs never create duplicates.
///
/// Returns `(added, skipped)` counts.
pub(super) fn import_issue_comments(
    conn: &Connection,
    task_uuid: &uuid::Uuid,
    comments: &[GhComment],
) -> Result<(usize, usize)> {
    let mut added = 0usize;
    let mut meta_comments: Vec<crate::infrastructure::model::GithubComment> =
        Vec::with_capacity(comments.len());

    for c in comments {
        let gh_comment = crate::infrastructure::model::GithubComment {
            comment_id: c.id,
            author: c.user.login.clone(),
            body: c.body.clone().unwrap_or_default(),
            url: c.html_url.clone(),
            created_at: c.created_at,
            updated_at: c.updated_at,
        };
        if db::upsert_github_comment_annotation(conn, task_uuid, &gh_comment)? {
            added += 1;
        }
        meta_comments.push(gh_comment);
    }

    let skipped = comments.len().saturating_sub(added);
    // Always refresh the full metadata so url/updated_at stay current.
    db::set_github_comments(conn, task_uuid, &meta_comments)?;
    Ok((added, skipped))
}
