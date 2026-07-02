//! Integration tests for `sara_tasks::commands::sync::github`.
//! Moved out of an inline mod tests block in src/commands/sync/github.rs.

use sara_tasks::commands::sync::github::{GhComment, GhIssue};

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
