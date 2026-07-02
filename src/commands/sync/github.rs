use anyhow::{Context, Result};
use chrono::Utc;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GhUser {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct GhIssueAssignee {
    pub login: String,
}

/// Minimal GitHub issue shape from the REST API.
#[derive(Debug, Deserialize)]
pub struct GhIssue {
    pub id: i64,
    pub node_id: Option<String>,
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub html_url: String,
    pub state: String,
    pub updated_at: chrono::DateTime<Utc>,
    pub user: GhUser,
    #[serde(default)]
    pub assignees: Vec<GhIssueAssignee>,
    /// Present only on pull requests; used to exclude them.
    pub pull_request: Option<serde_json::Value>,
}

/// Minimal GitHub issue comment shape from the REST API.
#[derive(Debug, Deserialize)]
pub struct GhComment {
    pub id: i64,
    pub body: Option<String>,
    pub html_url: String,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
    pub user: GhUser,
}

/// Fall back to the gh CLI's stored token via `gh auth token`.
///
/// Returns `None` (rather than erroring) when gh is missing or unauthenticated,
/// so the caller falls through to the explicit "no token found" error.
pub(super) fn gh_auth_token() -> Option<String> {
    let out = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if token.is_empty() { None } else { Some(token) }
}

/// Resolve the authenticated GitHub login via `gh api /user`.
pub(super) fn github_login(token: &str) -> Result<String> {
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
pub(super) fn fetch_assigned_issues(
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

/// Fetch all comments for a single issue (or PR) in `owner/repo`.
///
/// Uses `gh api --paginate` so every page is retrieved automatically.
pub(super) fn fetch_issue_comments(
    token: &str,
    owner: &str,
    repo: &str,
    issue_number: i64,
) -> Result<Vec<GhComment>> {
    let endpoint = format!("/repos/{owner}/{repo}/issues/{issue_number}/comments?per_page=100");

    let out = std::process::Command::new("gh")
        .env("GH_TOKEN", token)
        .args(["api", "--paginate", &endpoint])
        .output()
        .with_context(|| {
            format!("failed to call gh api for comments on {owner}/{repo}#{issue_number}")
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
