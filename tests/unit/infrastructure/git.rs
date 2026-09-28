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

#[test]
fn is_clean_detects_dirty_and_clean_trees() {
    let dir = test_dir("is-clean");
    make_git_repo_with_remote(&dir, None);
    std::fs::write(dir.join("a.txt"), "1").unwrap();
    std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(&dir)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(is_clean(&dir), Some(true), "committed tree is clean");

    std::fs::write(dir.join("a.txt"), "changed").unwrap();
    assert_eq!(is_clean(&dir), Some(false), "modified file → dirty");

    // Untracked files also count as dirty.
    std::process::Command::new("git")
        .args(["checkout", "--", "a.txt"])
        .current_dir(&dir)
        .output()
        .unwrap();
    std::fs::write(dir.join("new.txt"), "x").unwrap();
    assert_eq!(is_clean(&dir), Some(false), "untracked file → dirty");
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
    assert!(msg.contains("No 'origin' remote"), "unexpected: {msg}");
    assert!(msg.contains("Sara needs"), "unexpected: {msg}");
}

#[test]
fn github_repo_from_remote_fails_clearly_for_non_github_url() {
    let dir = test_dir("gh-remote-non-github");
    make_git_repo_with_remote(&dir, Some("https://gitlab.com/user/repo.git"));
    let err = github_repo_from_remote(&dir).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("not a recognised GitHub remote"),
        "unexpected: {msg}"
    );
    assert!(msg.contains("Sara expects"), "unexpected: {msg}");
}
