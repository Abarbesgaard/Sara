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

/// Resolve the GitHub `owner/repo` by reading the `origin` remote URL from the
/// given git repository root.
///
/// Errors with a message explaining what Sara expected when:
/// - The repository has no `origin` remote.
/// - The `origin` URL is not a recognised GitHub remote form.
pub fn github_repo_from_remote(repo_root: &Path) -> Result<(String, String)> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["remote", "get-url", "origin"])
        .output()
        .context("failed to run git remote get-url")?;

    if !out.status.success() {
        anyhow::bail!(
            "No 'origin' remote found in this repository. \
             Sara needs an 'origin' remote that points to a GitHub repository \
             (e.g. 'https://github.com/owner/repo.git' or 'git@github.com:owner/repo.git')."
        );
    }

    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    parse_github_owner_repo(&url).ok_or_else(|| {
        anyhow::anyhow!(
            "Remote 'origin' URL '{url}' is not a recognised GitHub remote. \
             Sara expects a URL like 'https://github.com/owner/repo.git' \
             or 'git@github.com:owner/repo.git'."
        )
    })
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
