use super::*;
use chrono::Utc;

#[test]
fn project_commands_round_trip_and_partial_update_preserves_others() {
    let conn = mem();
    set_project_commands(
        &conn,
        "demo",
        &ProjectCommands {
            setup_cmd: Some("cargo fetch".into()),
            test_cmd: Some("cargo test".into()),
            lint_cmd: None,
            run_cmd: None,
        },
    )
    .unwrap();
    set_project_commands(
        &conn,
        "demo",
        &ProjectCommands {
            setup_cmd: None,
            test_cmd: None,
            lint_cmd: Some("cargo clippy".into()),
            run_cmd: None,
        },
    )
    .unwrap();

    let c = get_project_commands(&conn, "demo").unwrap();
    assert_eq!(c.setup_cmd.as_deref(), Some("cargo fetch"));
    assert_eq!(c.test_cmd.as_deref(), Some("cargo test"));
    assert_eq!(c.lint_cmd.as_deref(), Some("cargo clippy"));
    assert!(c.run_cmd.is_none());
}

#[test]
fn get_project_commands_defaults_to_empty_when_absent() {
    let conn = mem();
    let c = get_project_commands(&conn, "nope").unwrap();
    assert!(c.setup_cmd.is_none() && c.test_cmd.is_none());
}

#[test]
fn github_sync_settings_round_trip_through_project_storage() {
    let conn = mem();
    upsert_project_seen(&conn, "myrepo", Some("/home/u/myrepo")).unwrap();
    set_github_sync(
        &conn,
        "myrepo",
        &GithubSyncSettings {
            repo: Some("acme/myrepo".into()),
            login: Some("alice".into()),
            scope: Some("issues".into()),
        },
    )
    .unwrap();

    let s = get_github_sync(&conn, "myrepo").unwrap();
    assert_eq!(s.repo.as_deref(), Some("acme/myrepo"));
    assert_eq!(s.login.as_deref(), Some("alice"));
    assert_eq!(s.scope.as_deref(), Some("issues"));
}

#[test]
fn save_project_profile_persists_github_fields() {
    let conn = mem();
    save_project_profile(
        &conn,
        &crate::infrastructure::model::Project {
            name: "myrepo".into(),
            path: Some("/home/u/myrepo".into()),
            goal: Some("g".into()),
            stack: None,
            conventions: None,
            notes: None,
            initialized_at: None,
            last_seen: None,
            github_repo: Some("acme/myrepo".into()),
            github_login: Some("alice".into()),
            github_sync_scope: Some("issues".into()),
        },
    )
    .unwrap();

    let project = get_project(&conn, "myrepo").unwrap().unwrap();
    assert_eq!(project.github_repo.as_deref(), Some("acme/myrepo"));
    assert_eq!(project.github_login.as_deref(), Some("alice"));
    assert_eq!(project.github_sync_scope.as_deref(), Some("issues"));
}

#[test]
fn github_sync_partial_update_preserves_existing_fields() {
    let conn = mem();
    upsert_project_seen(&conn, "p", None).unwrap();
    set_github_sync(
        &conn,
        "p",
        &GithubSyncSettings {
            repo: Some("org/p".into()),
            login: Some("bob".into()),
            scope: Some("issues".into()),
        },
    )
    .unwrap();
    set_github_sync(
        &conn,
        "p",
        &GithubSyncSettings {
            repo: None,
            login: None,
            scope: Some("issues,prs".into()),
        },
    )
    .unwrap();

    let s = get_github_sync(&conn, "p").unwrap();
    assert_eq!(s.repo.as_deref(), Some("org/p"), "repo preserved");
    assert_eq!(s.login.as_deref(), Some("bob"), "login preserved");
    assert_eq!(s.scope.as_deref(), Some("issues,prs"), "scope updated");
}

#[test]
fn github_sync_no_secret_field_in_settings_struct() {
    let s = GithubSyncSettings {
        repo: Some("org/repo".into()),
        login: Some("user".into()),
        scope: Some("issues".into()),
    };
    assert!(
        !s.login.as_deref().unwrap_or("").starts_with("ghp_"),
        "login field should hold a username, not a PAT"
    );
}

#[test]
fn project_detection_loads_github_sync_metadata_for_path() {
    let conn = mem();
    upsert_project_seen(&conn, "sara", Some("/home/u/Sara")).unwrap();
    set_github_sync(
        &conn,
        "sara",
        &GithubSyncSettings {
            repo: Some("acme/sara".into()),
            login: Some("alice".into()),
            scope: Some("issues".into()),
        },
    )
    .unwrap();

    let project = get_project_by_path(&conn, "/home/u/Sara")
        .unwrap()
        .expect("project must be found by path");

    assert_eq!(project.name, "sara");
    assert_eq!(project.github_repo.as_deref(), Some("acme/sara"));
    assert_eq!(project.github_login.as_deref(), Some("alice"));
    assert_eq!(project.github_sync_scope.as_deref(), Some("issues"));
}

#[test]
fn github_provenance_round_trips_through_meta_json() {
    let conn = mem();
    let task = seed_task(&conn);

    let prov = crate::infrastructure::model::GithubProvenance {
        repo: "acme/widgets".into(),
        issue_id: Some(42),
        node_id: Some("NODE42".into()),
        number: 99,
        html_url: Some("https://github.com/acme/widgets/issues/99".into()),
        title: Some("Fix widget".into()),
        body: Some("body".into()),
        state: Some("open".into()),
        assignees: vec!["alice".into()],
        creator: Some("alice".into()),
        updated_at: Some(Utc::now()),
        synced_at: Utc::now(),
        synced_by: Some("alice".into()),
    };
    set_github_provenance(&conn, &task.uuid, &prov).unwrap();

    let loaded = get_github_provenance(&conn, &task.uuid)
        .unwrap()
        .expect("provenance must be present");
    assert_eq!(loaded.repo, "acme/widgets");
    assert_eq!(loaded.number, 99);
    assert_eq!(loaded.synced_by.as_deref(), Some("alice"));
    assert_eq!(loaded.issue_id, Some(42));
    assert_eq!(loaded.node_id.as_deref(), Some("NODE42"));
}

#[test]
fn github_provenance_merges_with_existing_meta_json_keys() {
    let conn = mem();
    let task = seed_task(&conn);

    set_meta_json(&conn, &task.uuid, r#"{"my_key":"keep_me"}"#).unwrap();

    let prov = crate::infrastructure::model::GithubProvenance {
        repo: "org/repo".into(),
        issue_id: None,
        node_id: None,
        number: 1,
        html_url: Some("https://github.com/org/repo/issues/1".into()),
        title: Some("Issue".into()),
        body: None,
        state: Some("open".into()),
        assignees: vec![],
        creator: Some("alice".into()),
        updated_at: Some(Utc::now()),
        synced_at: Utc::now(),
        synced_by: None,
    };
    set_github_provenance(&conn, &task.uuid, &prov).unwrap();

    let raw = get_guide_fields(&conn, &task.uuid)
        .unwrap()
        .meta_json
        .unwrap();
    let obj: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(obj["my_key"], "keep_me");
    assert_eq!(obj["github"]["repo"], "org/repo");
    assert_eq!(obj["github"]["number"], 1);
}

#[test]
fn github_provenance_contains_no_secret_fields() {
    let prov = crate::infrastructure::model::GithubProvenance {
        repo: "org/repo".into(),
        issue_id: Some(7),
        node_id: Some("NODE7".into()),
        number: 5,
        html_url: Some("https://github.com/org/repo/issues/5".into()),
        title: Some("Issue".into()),
        body: Some("body".into()),
        state: Some("open".into()),
        assignees: vec!["bob".into()],
        creator: Some("bob".into()),
        updated_at: Some(Utc::now()),
        synced_at: Utc::now(),
        synced_by: Some("bob".into()),
    };
    let serialized = serde_json::to_string(&prov).unwrap();
    assert!(!serialized.to_lowercase().contains("token"));
    assert!(!serialized.to_lowercase().contains(r#""pat""#));
}

#[test]
fn find_github_task_uuid_matches_repo_and_number_or_node_id() {
    let conn = mem();
    let task = seed_task(&conn);
    let prov = crate::infrastructure::model::GithubProvenance {
        repo: "acme/widgets".into(),
        issue_id: Some(100),
        node_id: Some("NODE100".into()),
        number: 8,
        html_url: Some("https://github.com/acme/widgets/issues/8".into()),
        title: Some("Issue".into()),
        body: None,
        state: Some("open".into()),
        assignees: vec![],
        creator: Some("alice".into()),
        updated_at: Some(Utc::now()),
        synced_at: Utc::now(),
        synced_by: Some("alice".into()),
    };
    set_github_provenance(&conn, &task.uuid, &prov).unwrap();

    let by_number = find_github_task_uuid(&conn, "acme/widgets", 8, None)
        .unwrap()
        .expect("match by number");
    assert_eq!(by_number, task.uuid);

    let by_node = find_github_task_uuid(&conn, "acme/widgets", 999, Some("NODE100"))
        .unwrap()
        .expect("match by node id");
    assert_eq!(by_node, task.uuid);
}

#[test]
fn github_comment_annotation_is_inserted_once() {
    let conn = mem();
    let task = seed_task(&conn);
    let c = make_gh_comment(42, "alice", "Looks good");

    let first = upsert_github_comment_annotation(&conn, &task.uuid, &c).unwrap();
    assert!(first, "first insert should return true");

    let second = upsert_github_comment_annotation(&conn, &task.uuid, &c).unwrap();
    assert!(!second, "duplicate insert should return false");

    let anns = get_annotations(&conn, &task.uuid).unwrap();
    assert_eq!(anns.len(), 1, "only one annotation must exist");
    assert_eq!(anns[0].author, "alice");
    assert_eq!(anns[0].text, "Looks good");
    assert_eq!(
        anns[0].target_kind.as_deref(),
        Some(NOTE_KIND_GITHUB_COMMENT)
    );
    assert_eq!(anns[0].target_id.as_deref(), Some("42"));
}

#[test]
fn github_comment_kind_is_comment_for_info_visibility() {
    let conn = mem();
    let task = seed_task(&conn);
    let c = make_gh_comment(7, "bob", "Fix it");
    upsert_github_comment_annotation(&conn, &task.uuid, &c).unwrap();

    let anns = get_annotations(&conn, &task.uuid).unwrap();
    assert_eq!(anns[0].kind, "comment");
}

#[test]
fn github_comment_annotation_uses_github_created_at_as_entry() {
    use chrono::TimeZone;
    let conn = mem();
    let task = seed_task(&conn);
    let created = Utc.with_ymd_and_hms(2025, 3, 15, 8, 0, 0).unwrap();
    let mut c = make_gh_comment(10, "carol", "hello");
    c.created_at = created;
    upsert_github_comment_annotation(&conn, &task.uuid, &c).unwrap();

    let anns = get_annotations(&conn, &task.uuid).unwrap();
    assert_eq!(
        anns[0].entry.timestamp(),
        created.timestamp(),
        "entry must equal the comment's created_at"
    );
}

#[test]
fn github_comments_round_trip_through_meta_json() {
    let conn = mem();
    let task = seed_task(&conn);

    let comments = vec![
        make_gh_comment(1, "alice", "First comment"),
        make_gh_comment(2, "bob", "Second comment"),
    ];
    set_github_comments(&conn, &task.uuid, &comments).unwrap();

    let loaded = get_github_comments(&conn, &task.uuid).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].comment_id, 1);
    assert_eq!(loaded[0].author, "alice");
    assert_eq!(loaded[0].body, "First comment");
    assert_eq!(
        loaded[0].url,
        "https://github.com/a/b/issues/1#issuecomment-1"
    );
    assert_eq!(loaded[1].comment_id, 2);
}

#[test]
fn github_comments_meta_json_preserves_other_keys() {
    let conn = mem();
    let task = seed_task(&conn);

    set_meta_json(&conn, &task.uuid, r#"{"other_key":"keep_me"}"#).unwrap();
    set_github_comments(&conn, &task.uuid, &[make_gh_comment(5, "dave", "hi")]).unwrap();

    let raw = get_guide_fields(&conn, &task.uuid)
        .unwrap()
        .meta_json
        .unwrap();
    let obj: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        obj["other_key"], "keep_me",
        "existing keys must be preserved"
    );
    assert_eq!(obj["github_comments"][0]["comment_id"], 5);
}

#[test]
fn repeated_set_github_comments_replaces_array() {
    let conn = mem();
    let task = seed_task(&conn);

    set_github_comments(&conn, &task.uuid, &[make_gh_comment(1, "a", "old")]).unwrap();
    set_github_comments(
        &conn,
        &task.uuid,
        &[
            make_gh_comment(1, "a", "old"),
            make_gh_comment(2, "b", "new"),
        ],
    )
    .unwrap();

    let loaded = get_github_comments(&conn, &task.uuid).unwrap();
    assert_eq!(loaded.len(), 2, "array is replaced with the latest set");
}

#[test]
fn upsert_github_comment_idempotent_across_multiple_calls() {
    let conn = mem();
    let task = seed_task(&conn);
    let comments = vec![
        make_gh_comment(100, "alice", "LGTM"),
        make_gh_comment(101, "bob", "Please clarify"),
    ];

    for c in &comments {
        upsert_github_comment_annotation(&conn, &task.uuid, c).unwrap();
    }
    set_github_comments(&conn, &task.uuid, &comments).unwrap();

    for c in &comments {
        let inserted = upsert_github_comment_annotation(&conn, &task.uuid, c).unwrap();
        assert!(
            !inserted,
            "second sync must not re-insert comment {}",
            c.comment_id
        );
    }
    set_github_comments(&conn, &task.uuid, &comments).unwrap();

    let anns = get_annotations(&conn, &task.uuid).unwrap();
    assert_eq!(
        anns.len(),
        2,
        "no duplicate annotations after repeated sync"
    );

    let meta = get_github_comments(&conn, &task.uuid).unwrap();
    assert_eq!(meta.len(), 2);
}

#[test]
fn github_comment_metadata_preserves_url_and_updated_at() {
    let conn = mem();
    let task = seed_task(&conn);
    let mut c = make_gh_comment(77, "eve", "test");
    c.url = "https://github.com/org/repo/issues/3#issuecomment-77".to_string();
    c.updated_at = Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();

    set_github_comments(&conn, &task.uuid, &[c.clone()]).unwrap();
    let loaded = get_github_comments(&conn, &task.uuid).unwrap();

    assert_eq!(
        loaded[0].url,
        "https://github.com/org/repo/issues/3#issuecomment-77"
    );
    assert_eq!(loaded[0].updated_at, c.updated_at);
}
