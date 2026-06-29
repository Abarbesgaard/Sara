use anyhow::{Context, Result};
use chrono::Utc;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct GhUser {
    pub(super) login: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct GhIssueAssignee {
    pub(super) login: String,
}

/// Minimal GitHub issue shape from the REST API.
#[derive(Debug, Deserialize)]
pub(super) struct GhIssue {
    pub(super) id: i64,
    pub(super) node_id: Option<String>,
    pub(super) number: i64,
    pub(super) title: String,
    pub(super) body: Option<String>,
    pub(super) html_url: String,
    pub(super) state: String,
    pub(super) updated_at: chrono::DateTime<Utc>,
    pub(super) user: GhUser,
    #[serde(default)]
    pub(super) assignees: Vec<GhIssueAssignee>,
    /// Present only on pull requests; used to exclude them.
    pub(super) pull_request: Option<serde_json::Value>,
}

/// Minimal GitHub issue comment shape from the REST API.
#[derive(Debug, Deserialize)]
pub(super) struct GhComment {
    pub(super) id: i64,
    pub(super) body: Option<String>,
    pub(super) html_url: String,
    pub(super) created_at: chrono::DateTime<Utc>,
    pub(super) updated_at: chrono::DateTime<Utc>,
    pub(super) user: GhUser,
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            c.html_url,
            "https://github.com/a/b/issues/7#issuecomment-999"
        );
        assert_eq!(c.user.login, "bob");
        assert_eq!(c.created_at.to_rfc3339(), "2026-06-01T09:00:00+00:00");
        assert_eq!(c.updated_at.to_rfc3339(), "2026-06-02T10:30:00+00:00");
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
