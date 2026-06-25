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
        // In any git repo (including this one), default_base returns a non-empty string.
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let base = default_base(repo);
        assert!(!base.is_empty());
    }

    #[test]
    fn current_branch_in_repo() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // May be None in detached HEAD / CI, but should not panic.
        let _ = current_branch(repo);
    }
}
