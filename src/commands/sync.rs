use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, params};
use serde::Deserialize;

use crate::config::Config;
use crate::db;
use crate::model::Task;
use crate::project::find_git_root;

/// Minimal GitHub issue shape from the REST API.
#[derive(Debug, Deserialize)]
struct GhIssue {
    number: i64,
    title: String,
    body: Option<String>,
    html_url: String,
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

    // `gh api --paginate` writes each page as a separate JSON array.
    // Parse each line and merge the results.
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

    // Exclude pull requests (the issues API mixes both).
    Ok(issues
        .into_iter()
        .filter(|i| i.pull_request.is_none())
        .collect())
}

/// Return true if a task for this GitHub issue already exists in the database.
fn already_imported(conn: &Connection, repo: &str, number: i64) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks \
         WHERE json_extract(meta_json, '$.github.repo') = ?1 \
           AND json_extract(meta_json, '$.github.number') = ?2",
        params![repo, number],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

/// Sync open GitHub issues assigned to the authenticated user for the current repo.
pub fn run(conn: &Connection, cfg: &Config) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let git_root =
        find_git_root(&cwd).ok_or_else(|| anyhow::anyhow!("Not inside a git repository"))?;

    let (owner, repo) = github_repo_from_remote(&git_root)?;
    let login = github_login()?;
    let (project_name, _) = crate::project::detect_current_project(conn, cfg)?;

    println!("Syncing issues for {owner}/{repo} assigned to @{login}…");

    let issues = fetch_assigned_issues(&owner, &repo, &login)?;
    db::set_github_sync(
        conn,
        &project_name,
        &db::GithubSyncSettings {
            repo: Some(format!("{owner}/{repo}")),
            login: Some(login.clone()),
            scope: Some("issues".to_string()),
        },
    )?;

    let mut imported = 0usize;
    let mut skipped = 0usize;

    for issue in &issues {
        if already_imported(conn, &format!("{owner}/{repo}"), issue.number)? {
            skipped += 1;
            continue;
        }

        let mut task = Task::new(issue.title.clone(), project_name.clone());
        task.tags.push("github".to_string());
        task.urgency = db::compute_urgency(&task, &cfg.urgency, false, 0);

        db::insert_task(conn, &mut task)?;

        db::set_github_provenance(
            conn,
            &task.uuid,
            &crate::model::GithubProvenance {
                repo: format!("{owner}/{repo}"),
                number: issue.number,
                imported_at: Utc::now(),
                imported_by: Some(login.clone()),
            },
        )?;

        // Attach the issue URL as a clickable link.
        db::add_link(
            conn,
            &task.uuid,
            &issue.html_url,
            Some(&format!("#{}", issue.number)),
        )?;

        // Store the issue body as the assignment (the "why" for this task).
        if let Some(ref body) = issue.body {
            let body = body.trim();
            if !body.is_empty() {
                db::set_assignment(conn, &task.uuid, body)?;
            }
        }

        println!(
            "  Imported task {} [#{issue_num}]: {title}",
            task.id.unwrap_or(0),
            issue_num = issue.number,
            title = issue.title,
        );
        imported += 1;
    }

    println!(
        "Done. {imported} issue{} imported, {skipped} already present.",
        if imported == 1 { "" } else { "s" }
    );
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
        // After stripping .git the repo part is empty
        assert!(parse_github_owner_repo("https://github.com/owner/").is_none());
    }

    #[test]
    fn pr_field_presence_marks_entry_as_pr() {
        let json = r#"[
            {"number":1,"title":"Fix bug","body":null,"html_url":"https://github.com/a/b/issues/1","pull_request":null},
            {"number":2,"title":"Add feature","body":null,"html_url":"https://github.com/a/b/issues/2"}
        ]"#;
        let issues: Vec<GhIssue> = serde_json::from_str(json).unwrap();
        // Issue #1 has pull_request: null (not a PR), issue #2 has no pull_request field (not a PR).
        // A real PR would have pull_request: { ... some object ... }.
        let filtered: Vec<_> = issues
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .collect();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn pull_request_entries_are_excluded() {
        let json = r#"[
            {"number":10,"title":"Real issue","body":null,"html_url":"https://github.com/a/b/issues/10"},
            {"number":11,"title":"A pull request","body":null,"html_url":"https://github.com/a/b/pull/11","pull_request":{"url":"https://api.github.com/repos/a/b/pulls/11"}}
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
