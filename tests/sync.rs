//! Integration tests for `sara_tasks::commands::sync`.
//! Moved out of an inline mod tests block in src/commands/sync/mod.rs.

use sara_tasks::commands::sync::resolve_github_token_from;

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
