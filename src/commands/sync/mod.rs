use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::git::github_repo_from_remote;
use crate::infrastructure::project::find_git_root;

mod github;
mod import;

/// Resolve a GitHub token for the sync API calls.
///
/// Precedence: `GH_TOKEN` env > `GITHUB_TOKEN` env > the gh CLI's stored token
/// (`gh auth token`). The last step means a user who has run `gh auth login`
/// does not have to export a token by hand — and `gh` is already a hard
/// dependency of sync. The error explains both paths when nothing is found.
pub fn resolve_github_token() -> Result<String> {
    resolve_github_token_from(|key| std::env::var(key).ok(), github::gh_auth_token)
}

fn resolve_github_token_from<F, G>(mut lookup: F, gh_token: G) -> Result<String>
where
    F: FnMut(&str) -> Option<String>,
    G: FnOnce() -> Option<String>,
{
    if let Some(token) = lookup("GH_TOKEN").filter(|t| !t.trim().is_empty()) {
        return Ok(token);
    }
    if let Some(token) = lookup("GITHUB_TOKEN").filter(|t| !t.trim().is_empty()) {
        return Ok(token);
    }
    if let Some(token) = gh_token().filter(|t| !t.trim().is_empty()) {
        return Ok(token);
    }

    anyhow::bail!(
        "No GitHub token found. Authenticate the gh CLI with 'gh auth login', \
         or export GH_TOKEN or GITHUB_TOKEN in your shell, for example:\n\
         export GH_TOKEN=ghp_your_token_here\n\
         then launch Sara again."
    )
}

/// Sync open GitHub issues assigned to the authenticated user for the current repo.
pub fn run(conn: &Connection, cfg: &Config) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let git_root =
        find_git_root(&cwd).ok_or_else(|| anyhow::anyhow!("Not inside a git repository"))?;

    let (owner, repo) = github_repo_from_remote(&git_root)?;
    let token = resolve_github_token()?;
    let login = github::github_login(&token)?;
    let (project_name, _) = crate::infrastructure::project::detect_current_project(conn, cfg)?;
    let repo_full_name = format!("{owner}/{repo}");

    println!("Syncing issues for {owner}/{repo} assigned to @{login}…");

    let issues = github::fetch_assigned_issues(&token, &owner, &repo, &login)?;
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
            import::update_existing_task(conn, cfg, &task_uuid, issue, &repo_full_name, &login)?;
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
                import::create_new_task(conn, cfg, &project_name, &repo_full_name, &login, issue)?;
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

        let raw_comments = github::fetch_issue_comments(&token, &owner, &repo, issue.number)?;
        let (new_comments, _) = import::import_issue_comments(conn, &task_uuid, &raw_comments)?;
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
        let token = resolve_github_token_from(
            |key| match key {
                "GH_TOKEN" => Some("gh-token".into()),
                "GITHUB_TOKEN" => Some("github-token".into()),
                _ => None,
            },
            || Some("gh-cli-token".into()),
        )
        .unwrap();
        assert_eq!(token, "gh-token");
    }

    #[test]
    fn falls_back_to_github_token_when_gh_token_absent() {
        let token = resolve_github_token_from(
            |key| match key {
                "GH_TOKEN" => None,
                "GITHUB_TOKEN" => Some("github-token".into()),
                _ => None,
            },
            || Some("gh-cli-token".into()),
        )
        .unwrap();
        assert_eq!(token, "github-token");
    }

    #[test]
    fn falls_back_to_gh_auth_token_when_env_absent() {
        let token = resolve_github_token_from(|_| None, || Some("gh-cli-token".into())).unwrap();
        assert_eq!(token, "gh-cli-token");
    }

    #[test]
    fn env_token_wins_over_gh_auth_token() {
        let token = resolve_github_token_from(
            |key| (key == "GH_TOKEN").then(|| "gh-token".into()),
            || Some("gh-cli-token".into()),
        )
        .unwrap();
        assert_eq!(token, "gh-token");
    }

    #[test]
    fn fails_with_clear_error_when_no_token_anywhere() {
        let err = resolve_github_token_from(|_| None, || None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("GH_TOKEN"), "{msg}");
        assert!(msg.contains("GITHUB_TOKEN"), "{msg}");
        assert!(msg.contains("gh auth login"), "{msg}");
    }
}
