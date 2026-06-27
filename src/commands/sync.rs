use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::Connection;
use serde::Deserialize;

use crate::config::Config;
use crate::db;
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

/// Parse "owner" and "repo" from a GitHub remote URL.
///
/// Supports:
///   - SSH:   `git@github.com:owner/repo.git`
///   - HTTPS: `https://github.com/owner/repo[.git]`
fn parse_github_owner_repo(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let stripped = url
        .strip_prefix("git@github.com:")
        .or_else(|| url.strip_prefix("https://github.com/"))
        .or_else(|| url.strip_prefix("http://github.com/"))?;

    let stripped = stripped.strip_suffix(".git").unwrap_or(stripped);
    let mut parts = stripped.splitn(2, '/');
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.to_string();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner, repo))
}

/// Resolve the GitHub owner/repo by reading the `origin` remote URL.
fn github_repo_from_remote(repo_root: &std::path::Path) -> Result<(String, String)> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["remote", "get-url", "origin"])
        .output()
        .context("failed to run git remote get-url")?;

    if !out.status.success() {
        anyhow::bail!("No 'origin' remote found. Is this repository hosted on GitHub?");
    }

    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    parse_github_owner_repo(&url)
        .ok_or_else(|| anyhow::anyhow!("Remote URL '{url}' does not look like a GitHub repo"))
}

/// Resolve the authenticated GitHub login via `gh api /user`.
fn github_login() -> Result<String> {
    let out = std::process::Command::new("gh")
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
fn fetch_assigned_issues(owner: &str, repo: &str, login: &str) -> Result<Vec<GhIssue>> {
    let endpoint = format!("/repos/{owner}/{repo}/issues?state=open&assignee={login}&per_page=100");

    let out = std::process::Command::new("gh")
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

/// Sync open GitHub issues assigned to the authenticated user for the current repo.
pub fn run(conn: &Connection, cfg: &Config) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let git_root =
        find_git_root(&cwd).ok_or_else(|| anyhow::anyhow!("Not inside a git repository"))?;

    let (owner, repo) = github_repo_from_remote(&git_root)?;
    let login = github_login()?;
    let (project_name, _) = crate::project::detect_current_project(conn, cfg)?;
    let repo_full_name = format!("{owner}/{repo}");

    println!("Syncing issues for {owner}/{repo} assigned to @{login}…");

    let issues = fetch_assigned_issues(&owner, &repo, &login)?;
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
        if let Some(task_uuid) = existing {
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
        }
    }

    println!("Done. {created} created, {updated} updated.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssh_remote_url() {
        let (o, r) = parse_github_owner_repo("git@github.com:Abarbesgaard/Sara.git").unwrap();
        assert_eq!(o, "Abarbesgaard");
        assert_eq!(r, "Sara");
    }

    #[test]
    fn parses_https_url_with_git_suffix() {
        let (o, r) = parse_github_owner_repo("https://github.com/Abarbesgaard/Sara.git").unwrap();
        assert_eq!(o, "Abarbesgaard");
        assert_eq!(r, "Sara");
    }

    #[test]
    fn parses_https_url_without_git_suffix() {
        let (o, r) = parse_github_owner_repo("https://github.com/Abarbesgaard/Sara").unwrap();
        assert_eq!(o, "Abarbesgaard");
        assert_eq!(r, "Sara");
    }

    #[test]
    fn parses_http_url() {
        let (o, r) = parse_github_owner_repo("http://github.com/owner/repo.git").unwrap();
        assert_eq!(o, "owner");
        assert_eq!(r, "repo");
    }

    #[test]
    fn rejects_non_github_url() {
        assert!(parse_github_owner_repo("https://gitlab.com/user/repo.git").is_none());
    }

    #[test]
    fn rejects_url_with_empty_owner() {
        assert!(parse_github_owner_repo("git@github.com:/repo.git").is_none());
    }

    #[test]
    fn rejects_url_with_empty_repo() {
        assert!(parse_github_owner_repo("https://github.com/owner/").is_none());
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
}
