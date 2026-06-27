use anyhow::{Context, Result};
use std::path::Path;

/// Run `git -C <repo>` with the given args. Returns trimmed stdout or an error.
fn git_output(repo: &Path, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .context("failed to run git")?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        anyhow::bail!(
            "{}",
            if stderr.is_empty() {
                "git command failed".to_string()
            } else {
                stderr
            }
        )
    }
}

/// Return the currently checked-out branch name, or None if detached HEAD / not a repo.
pub fn current_branch(repo: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if branch == "HEAD" {
        None // detached HEAD
    } else {
        Some(branch)
    }
}

/// Return the current HEAD commit SHA (short), or None if not a repo.
pub fn head_commit(repo: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() { None } else { Some(sha) }
}

/// Heuristic: find the most likely base branch for comparison.
/// Prefers the default remote branch, then falls back to main/master.
pub fn default_base(repo: &Path) -> String {
    // Try remote HEAD symbolic ref
    if let Ok(out) = git_output(
        repo,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) && !out.is_empty()
    {
        return out; // e.g. "origin/main"
    }
    // Fall back to first existing of main / master (local)
    for candidate in ["main", "master"] {
        if git_output(repo, &["rev-parse", "--verify", candidate]).is_ok() {
            return candidate.to_string();
        }
    }
    "main".to_string()
}

/// Parse "owner" and "repo" from a GitHub remote URL.
///
/// Supports:
///   - SSH:   `git@github.com:owner/repo.git`
///   - HTTPS: `https://github.com/owner/repo[.git]`
///   - HTTP:  `http://github.com/owner/repo[.git]`
pub fn parse_github_owner_repo(url: &str) -> Option<(String, String)> {
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

/// Resolve the GitHub `owner/repo` by finding a GitHub remote in the
/// given git repository root.
///
/// Searches all remotes for GitHub URLs. If multiple GitHub remotes exist,
/// uses the first one found (sorted alphabetically by remote name).
///
/// Errors with a message explaining what Sara expected when:
/// - The repository has no remotes.
/// - None of the remotes point to GitHub.
pub fn github_repo_from_remote(repo_root: &Path) -> Result<(String, String)> {
    // Get all remotes with their URLs
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["remote", "-v"])
        .output()
        .context("failed to run git remote -v")?;

    if !out.status.success() {
        anyhow::bail!("Failed to list git remotes.");
    }

    let remotes_output = String::from_utf8_lossy(&out.stdout);
    let mut github_remotes: Vec<(String, String, String)> = Vec::new();

    // Parse remotes and find GitHub URLs
    for line in remotes_output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let remote_name = parts[0];
            let url = parts[1];
            
            if let Some((owner, repo)) = parse_github_owner_repo(url) {
                github_remotes.push((remote_name.to_string(), owner, repo));
            }
        }
    }

    if github_remotes.is_empty() {
        anyhow::bail!(
            "No GitHub remotes found in this repository. \
             Sara needs a remote that points to a GitHub repository \
             (e.g. 'https://github.com/owner/repo.git' or 'git@github.com:owner/repo.git')."
        );
    }

    // Deduplicate by remote name (git remote -v shows each remote twice: fetch/push)
    github_remotes.sort_by(|a, b| a.0.cmp(&b.0));
    github_remotes.dedup_by(|a, b| a.0 == b.0);

    // Use the first GitHub remote (alphabetically)
    let (remote_name, owner, repo) = &github_remotes[0];
    if github_remotes.len() > 1 {
        eprintln!("Multiple GitHub remotes found. Using '{remote_name}': {owner}/{repo}");
    }
    
    Ok((owner.clone(), repo.clone()))
}

/// Return `(base_ref, changed_file_paths)` for the given branch relative to
/// the auto-detected base. Uses three-dot diff (since merge-base).
pub fn changed_files(repo: &Path, branch: &str) -> Result<(String, Vec<String>)> {
    // Verify branch exists
    git_output(repo, &["rev-parse", "--verify", branch])
        .with_context(|| format!("branch '{}' not found", branch))?;

    let base = default_base(repo);

    if branch == base {
        return Ok((base, vec![]));
    }

    let diff_range = format!("{}...{}", base, branch);
    let raw = git_output(repo, &["diff", "--name-only", &diff_range])
        .with_context(|| format!("git diff failed for range {}", diff_range))?;

    let files = raw
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Ok((base, files))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_base_falls_back_gracefully() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let base = default_base(repo);
        assert!(!base.is_empty());
    }

    #[test]
    fn current_branch_in_repo() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let _ = current_branch(repo);
    }

    // --- parse_github_owner_repo ---

    #[test]
    fn parses_ssh_remote_url() {
        let (o, r) = parse_github_owner_repo("git@github.com:owner/repo.git").unwrap();
        assert_eq!(o, "owner");
        assert_eq!(r, "repo");
    }

    #[test]
    fn parses_https_url_with_git_suffix() {
        let (o, r) = parse_github_owner_repo("https://github.com/owner/repo.git").unwrap();
        assert_eq!(o, "owner");
        assert_eq!(r, "repo");
    }

    #[test]
    fn parses_https_url_without_git_suffix() {
        let (o, r) = parse_github_owner_repo("https://github.com/owner/repo").unwrap();
        assert_eq!(o, "owner");
        assert_eq!(r, "repo");
    }

    #[test]
    fn parses_http_url() {
        let (o, r) = parse_github_owner_repo("http://github.com/owner/repo.git").unwrap();
        assert_eq!(o, "owner");
        assert_eq!(r, "repo");
    }

    #[test]
    fn parses_url_with_surrounding_whitespace() {
        let (o, r) = parse_github_owner_repo("  git@github.com:owner/repo.git\n").unwrap();
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

    // --- github_repo_from_remote ---

    fn make_git_repo_with_remote(dir: &std::path::Path, remote_url: Option<&str>) {
        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .unwrap();
        if let Some(url) = remote_url {
            std::process::Command::new("git")
                .args(["remote", "add", "origin", url])
                .current_dir(dir)
                .output()
                .unwrap();
        }
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-git-repos")
            .join(name);
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn github_repo_from_remote_resolves_ssh_origin() {
        let dir = test_dir("gh-remote-ssh");
        make_git_repo_with_remote(&dir, Some("git@github.com:testowner/testrepo.git"));
        let (owner, repo) = github_repo_from_remote(&dir).unwrap();
        assert_eq!(owner, "testowner");
        assert_eq!(repo, "testrepo");
    }

    #[test]
    fn github_repo_from_remote_resolves_https_origin() {
        let dir = test_dir("gh-remote-https");
        make_git_repo_with_remote(&dir, Some("https://github.com/testowner/testrepo.git"));
        let (owner, repo) = github_repo_from_remote(&dir).unwrap();
        assert_eq!(owner, "testowner");
        assert_eq!(repo, "testrepo");
    }

    #[test]
    fn github_repo_from_remote_fails_clearly_when_no_origin() {
        let dir = test_dir("gh-remote-no-origin");
        make_git_repo_with_remote(&dir, None);
        let err = github_repo_from_remote(&dir).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("No GitHub remotes found"), "unexpected: {msg}");
        assert!(msg.contains("Sara needs"), "unexpected: {msg}");
    }

    #[test]
    fn github_repo_from_remote_fails_clearly_for_non_github_url() {
        let dir = test_dir("gh-remote-non-github");
        make_git_repo_with_remote(&dir, Some("https://gitlab.com/user/repo.git"));
        let err = github_repo_from_remote(&dir).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("No GitHub remotes found"),
            "unexpected: {msg}"
        );
        assert!(msg.contains("Sara needs"), "unexpected: {msg}");
    }
}
