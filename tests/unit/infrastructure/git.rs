use super::*;
use crate::test_support::{commit_all, git, git_repo};

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

#[test]
fn is_clean_detects_dirty_and_clean_trees() {
    let repo = repo_with_remote(None);
    let dir = repo.path();
    std::fs::write(dir.join("a.txt"), "1").unwrap();
    commit_all(dir, "init");
    assert_eq!(is_clean(dir), Some(true), "committed tree is clean");

    std::fs::write(dir.join("a.txt"), "changed").unwrap();
    assert_eq!(is_clean(dir), Some(false), "modified file → dirty");

    git(dir, &["checkout", "--", "a.txt"]);
    std::fs::write(dir.join("new.txt"), "x").unwrap();
    assert_eq!(is_clean(dir), Some(false), "untracked file → dirty");
}

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

fn repo_with_remote(remote_url: Option<&str>) -> tempfile::TempDir {
    let repo = git_repo();
    if let Some(url) = remote_url {
        git(repo.path(), &["remote", "add", "origin", url]);
    }
    repo
}

#[test]
fn github_repo_from_remote_resolves_ssh_origin() {
    let repo = repo_with_remote(Some("git@github.com:testowner/testrepo.git"));
    let dir = repo.path();
    let (owner, repo) = github_repo_from_remote(dir).unwrap();
    assert_eq!(owner, "testowner");
    assert_eq!(repo, "testrepo");
}

#[test]
fn github_repo_from_remote_resolves_https_origin() {
    let repo = repo_with_remote(Some("https://github.com/testowner/testrepo.git"));
    let dir = repo.path();
    let (owner, repo) = github_repo_from_remote(dir).unwrap();
    assert_eq!(owner, "testowner");
    assert_eq!(repo, "testrepo");
}

#[test]
fn github_repo_from_remote_fails_clearly_when_no_origin() {
    let repo = repo_with_remote(None);
    let dir = repo.path();
    let err = github_repo_from_remote(dir).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("No 'origin' remote"), "unexpected: {msg}");
    assert!(msg.contains("Sara needs"), "unexpected: {msg}");
}

#[test]
fn github_repo_from_remote_fails_clearly_for_non_github_url() {
    let repo = repo_with_remote(Some("https://gitlab.com/user/repo.git"));
    let dir = repo.path();
    let err = github_repo_from_remote(dir).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("not a recognised GitHub remote"),
        "unexpected: {msg}"
    );
    assert!(msg.contains("Sara expects"), "unexpected: {msg}");
}
