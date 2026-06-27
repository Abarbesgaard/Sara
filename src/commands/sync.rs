use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::Connection;
use serde::Deserialize;

use crate::config::Config;
use crate::db;
use crate::git::github_repo_from_remote;
use crate::model::Task;
use crate::project::find_git_root;

#[derive(Debug, Deserialize)]
struct GhUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GhIssueAssignee {
    login: String,
}

/// Minimal GitHub issue shape from the REST API.
#[derive(Debug, Deserialize)]
struct GhIssue {
    id: i64,
    node_id: Option<String>,
    number: i64,
    title: String,
    body: Option<String>,
    html_url: String,
    state: String,
    updated_at: chrono::DateTime<Utc>,
    user: GhUser,
    #[serde(default)]
    assignees: Vec<GhIssueAssignee>,
    /// Present only on pull requests; used to exclude them.
    pull_request: Option<serde_json::Value>,
}

/// Minimal GitHub issue comment shape from the REST API.
#[derive(Debug, Deserialize)]
struct GhComment {
    id: i64,
    body: Option<String>,
    html_url: String,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
    user: GhUser,
}

/// Resolve the GitHub token from the active shell environment.
///
/// `GH_TOKEN` wins over `GITHUB_TOKEN`. The error explains how to export a
/// token before launching Sara.
pub fn resolve_github_token() -> Result<String> {
    resolve_github_token_from(|key| std::env::var(key).ok())
}

fn resolve_github_token_from<F>(mut lookup: F) -> Result<String>
where
    F: FnMut(&str) -> Option<String>,
{
    if let Some(token) = lookup("GH_TOKEN").filter(|t| !t.trim().is_empty()) {
        return Ok(token);
    }
    if let Some(token) = lookup("GITHUB_TOKEN").filter(|t| !t.trim().is_empty()) {
        return Ok(token);
    }

    anyhow::bail!(
        "No GitHub token found in GH_TOKEN or GITHUB_TOKEN. \
         Export one in your shell first, for example:\n\
         export GH_TOKEN=ghp_your_token_here\n\
         then launch Sara again."
    )
}

/// Resolve the authenticated GitHub login via `gh api /user`.
fn github_login(token: &str) -> Result<String> {
    let out = std::process::Command::new("gh")
        .env("GH_TOKEN", token)
        .args(["api", "/user", "--jq", ".login"])
        .output()
        .context("failed to run 'gh api /user' — is 'gh' installed and authenticated?")?;

    if out.status.success() {
        let login = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !login.is_empty() {
            return Ok(login);
        }
    }

    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    anyhow::bail!("Could not resolve GitHub login: {stderr}. Run 'gh auth login' first.")
}

/// Fetch all open issues assigned to `login` for `owner/repo`.
///
/// Uses `gh api --paginate` so every page is retrieved automatically.
/// Pull requests (which the issues API also returns) are filtered out
/// by checking for the `pull_request` field.
fn fetch_assigned_issues(
    token: &str,
    owner: &str,
    repo: &str,
    login: &str,
) -> Result<Vec<GhIssue>> {
    let endpoint = format!("/repos/{owner}/{repo}/issues?state=open&assignee={login}&per_page=100");

    let out = std::process::Command::new("gh")
        .env("GH_TOKEN", token)
        .args(["api", "--paginate", &endpoint])
        .output()
        .with_context(|| format!("failed to call gh api for {owner}/{repo}"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        anyhow::bail!("GitHub API call failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut issues: Vec<GhIssue> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let batch: Vec<GhIssue> =
            serde_json::from_str(line).with_context(|| "failed to parse GitHub API response")?;
        issues.extend(batch);
    }

    Ok(issues
        .into_iter()
        .filter(|i| i.pull_request.is_none())
        .collect())
}

fn issue_provenance(
    repo: &str,
    sync_login: &str,
    issue: &GhIssue,
) -> crate::model::GithubProvenance {
    crate::model::GithubProvenance {
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

fn ensure_issue_link(conn: &Connection, task_uuid: &uuid::Uuid, issue: &GhIssue) -> Result<()> {
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

fn update_existing_task(
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

fn create_new_task(
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

/// Fetch all comments for a single issue (or PR) in `owner/repo`.
///
/// Uses `gh api --paginate` so every page is retrieved automatically.
fn fetch_issue_comments(
    token: &str,
    owner: &str,
    repo: &str,
    issue_number: i64,
) -> Result<Vec<GhComment>> {
    let endpoint =
        format!("/repos/{owner}/{repo}/issues/{issue_number}/comments?per_page=100");

    let out = std::process::Command::new("gh")
        .env("GH_TOKEN", token)
        .args(["api", "--paginate", &endpoint])
        .output()
        .with_context(|| {
            format!(
                "failed to call gh api for comments on {owner}/{repo}#{issue_number}"
            )
        })?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        anyhow::bail!("GitHub API call failed for comments: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut comments: Vec<GhComment> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let batch: Vec<GhComment> = serde_json::from_str(line)
            .with_context(|| "failed to parse GitHub comment API response")?;
        comments.extend(batch);
    }
    Ok(comments)
}

/// Import comments for a single issue into the task's annotation list.
///
/// Each comment is stored idempotently: the stable `comment_id` is used as the
/// deduplication key so repeated syncs never create duplicates.  The full
/// comment record (including `url` and `updated_at`) is also persisted in
/// `meta_json["github_comments"]`.
///
/// Returns `(added, skipped)` counts.
fn import_issue_comments(
    conn: &Connection,
    task_uuid: &uuid::Uuid,
    comments: &[GhComment],
) -> Result<(usize, usize)> {
    let mut added = 0usize;
    let mut meta_comments: Vec<crate::model::GithubComment> = Vec::with_capacity(comments.len());

    for c in comments {
        let gh_comment = crate::model::GithubComment {
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

/// Sync open GitHub issues assigned to the authenticated user for the current repo.
pub fn run(conn: &Connection, cfg: &Config) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let git_root =
        find_git_root(&cwd).ok_or_else(|| anyhow::anyhow!("Not inside a git repository"))?;

    let (owner, repo) = github_repo_from_remote(&git_root)?;
    let token = resolve_github_token()?;
    let login = github_login(&token)?;
    let (project_name, _) = crate::project::detect_current_project(conn, cfg)?;
    let repo_full_name = format!("{owner}/{repo}");

    println!("Syncing issues for {owner}/{repo} assigned to @{login}…");

    let issues = fetch_assigned_issues(&token, &owner, &repo, &login)?;
    db::set_github_sync(
        conn,
        &project_name,
        &db::GithubSyncSettings {
            repo: Some(repo_full_name.clone()),
            login: Some(login.clone()),
            scope: Some("issues".to_string()),
        },
    )?;

    let mut created = 0usize;
    let mut updated = 0usize;
    for issue in &issues {
        let existing = db::find_github_task_uuid(
            conn,
            &repo_full_name,
            issue.number,
            issue.node_id.as_deref(),
        )?;
        let task_uuid = if let Some(task_uuid) = existing {
            update_existing_task(conn, cfg, &task_uuid, issue, &repo_full_name, &login)?;
            let task = db::get_task_by_uuid_prefix(conn, &task_uuid.to_string())?
                .ok_or_else(|| anyhow::anyhow!("missing updated task {task_uuid}"))?;
            println!(
                "  Updated task {} [#{}]: {}",
                task.id.unwrap_or(0),
                issue.number,
                issue.title
            );
            updated += 1;
            task_uuid
        } else {
            let task_uuid =
                create_new_task(conn, cfg, &project_name, &repo_full_name, &login, issue)?;
            let task = db::get_task_by_uuid_prefix(conn, &task_uuid.to_string())?
                .ok_or_else(|| anyhow::anyhow!("missing created task {task_uuid}"))?;
            println!(
                "  Imported task {} [#{}]: {}",
                task.id.unwrap_or(0),
                issue.number,
                issue.title
            );
            created += 1;
            task_uuid
        };

        let raw_comments = fetch_issue_comments(&token, &owner, &repo, issue.number)?;
        let (new_comments, _) = import_issue_comments(conn, &task_uuid, &raw_comments)?;
        if new_comments > 0 {
            println!("    + {new_comments} new comment(s) imported");
        }
    }

    println!("Done. {created} created, {updated} updated.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gh_token_takes_precedence_over_github_token() {
        let token = resolve_github_token_from(|key| match key {
            "GH_TOKEN" => Some("gh-token".into()),
            "GITHUB_TOKEN" => Some("github-token".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(token, "gh-token");
    }

    #[test]
    fn falls_back_to_github_token_when_gh_token_absent() {
        let token = resolve_github_token_from(|key| match key {
            "GH_TOKEN" => None,
            "GITHUB_TOKEN" => Some("github-token".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(token, "github-token");
    }

    #[test]
    fn fails_with_clear_error_when_no_token_in_env() {
        let err = resolve_github_token_from(|_| None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("GH_TOKEN"), "{msg}");
        assert!(msg.contains("GITHUB_TOKEN"), "{msg}");
        assert!(msg.contains("export GH_TOKEN"), "{msg}");
    }

    #[test]
    fn issue_payload_deserialises_with_identity_fields() {
        let json = r#"{
            "id": 1,
            "node_id": "NODE1",
            "number": 7,
            "title": "Fix bug",
            "body": "body",
            "html_url": "https://github.com/a/b/issues/7",
            "state": "open",
            "updated_at": "2026-06-27T11:00:00Z",
            "user": {"login": "alice"},
            "assignees": [{"login": "alice"}]
        }"#;
        let issue: GhIssue = serde_json::from_str(json).unwrap();
        assert_eq!(issue.id, 1);
        assert_eq!(issue.node_id.as_deref(), Some("NODE1"));
        assert_eq!(issue.number, 7);
        assert_eq!(issue.user.login, "alice");
        assert_eq!(issue.assignees.len(), 1);
    }

    #[test]
    fn pr_field_presence_marks_entry_as_pr() {
        let json = r#"[
            {"id":1,"number":1,"title":"Fix bug","body":null,"html_url":"https://github.com/a/b/issues/1","state":"open","updated_at":"2026-06-27T11:00:00Z","user":{"login":"alice"},"assignees":[],"pull_request":null},
            {"id":2,"number":2,"title":"Add feature","body":null,"html_url":"https://github.com/a/b/issues/2","state":"open","updated_at":"2026-06-27T11:00:00Z","user":{"login":"bob"},"assignees":[]}
        ]"#;
        let issues: Vec<GhIssue> = serde_json::from_str(json).unwrap();
        let filtered: Vec<_> = issues
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .collect();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn pull_request_entries_are_excluded() {
        let json = r#"[
            {"id":10,"number":10,"title":"Real issue","body":null,"html_url":"https://github.com/a/b/issues/10","state":"open","updated_at":"2026-06-27T11:00:00Z","user":{"login":"alice"},"assignees":[]},
            {"id":11,"number":11,"title":"A pull request","body":null,"html_url":"https://github.com/a/b/pull/11","state":"open","updated_at":"2026-06-27T11:00:00Z","user":{"login":"bob"},"assignees":[],"pull_request":{"url":"https://api.github.com/repos/a/b/pulls/11"}}
        ]"#;
        let issues: Vec<GhIssue> = serde_json::from_str(json).unwrap();
        let filtered: Vec<_> = issues
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .collect();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].number, 10);
    }

    #[test]
    fn comment_payload_deserialises_with_all_required_fields() {
        let json = r#"{
            "id": 999,
            "body": "Great issue!",
            "html_url": "https://github.com/a/b/issues/7#issuecomment-999",
            "created_at": "2026-06-01T09:00:00Z",
            "updated_at": "2026-06-02T10:30:00Z",
            "user": {"login": "bob"}
        }"#;
        let c: GhComment = serde_json::from_str(json).unwrap();
        assert_eq!(c.id, 999);
        assert_eq!(c.body.as_deref(), Some("Great issue!"));
        assert_eq!(c.html_url, "https://github.com/a/b/issues/7#issuecomment-999");
        assert_eq!(c.user.login, "bob");
        assert_eq!(
            c.created_at.to_rfc3339(),
            "2026-06-01T09:00:00+00:00"
        );
        assert_eq!(
            c.updated_at.to_rfc3339(),
            "2026-06-02T10:30:00+00:00"
        );
    }

    #[test]
    fn comment_payload_handles_null_body() {
        let json = r#"{
            "id": 1,
            "body": null,
            "html_url": "https://github.com/a/b/issues/1#issuecomment-1",
            "created_at": "2026-06-01T00:00:00Z",
            "updated_at": "2026-06-01T00:00:00Z",
            "user": {"login": "alice"}
        }"#;
        let c: GhComment = serde_json::from_str(json).unwrap();
        assert!(c.body.is_none());
    }
}
