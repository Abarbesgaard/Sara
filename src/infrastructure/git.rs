use anyhow::{Context, Result};
use std::path::Path;

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
    if branch == "HEAD" { None } else { Some(branch) }
}

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

pub fn is_clean(repo: &Path) -> Option<bool> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["status", "--porcelain"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

pub fn default_base(repo: &Path) -> String {
    if let Ok(out) = git_output(
        repo,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) && !out.is_empty()
    {
        return out;
    }
    for candidate in ["main", "master"] {
        if git_output(repo, &["rev-parse", "--verify", candidate]).is_ok() {
            return candidate.to_string();
        }
    }
    "main".to_string()
}

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

#[cfg(test)]
#[path = "../../tests/unit/infrastructure/git.rs"]
mod tests;
