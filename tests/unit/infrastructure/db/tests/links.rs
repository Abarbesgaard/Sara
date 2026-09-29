use super::*;

#[test]
fn github_pr_url_gets_nice_label() {
    assert_eq!(
        derive_link_label("https://github.com/acme/widgets/pull/42"),
        Some("PR #42 · acme/widgets".to_string())
    );
    assert_eq!(
        derive_link_label("https://github.com/acme/widgets/issues/7"),
        Some("Issue #7 · acme/widgets".to_string())
    );
    assert_eq!(derive_link_label("https://example.com/foo"), None);
}

#[test]
fn is_issue_link_distinguishes_issues_from_prs_and_others() {
    assert!(is_issue_link("https://github.com/acme/widgets/issues/7"));
    assert!(!is_issue_link("https://github.com/acme/widgets/pull/42"));
    assert!(!is_issue_link("https://example.com/foo"));
}

#[test]
fn is_pr_link_distinguishes_prs_from_issues_and_others() {
    assert!(is_pr_link("https://github.com/acme/widgets/pull/42"));
    assert!(!is_pr_link("https://github.com/acme/widgets/issues/7"));
    assert!(!is_pr_link("https://example.com/foo"));
}

#[test]
fn link_flags_by_task_distinguishes_pr_issue_and_generic_links() {
    let conn = mem();
    let pr_task = seed_task(&conn);
    let issue_task = seed_task(&conn);
    let generic_task = seed_task(&conn);

    add_link(
        &conn,
        &pr_task.uuid,
        "https://github.com/acme/widgets/pull/42",
        None,
    )
    .unwrap();
    add_link(
        &conn,
        &issue_task.uuid,
        "https://github.com/acme/widgets/issues/7",
        None,
    )
    .unwrap();
    add_link(&conn, &generic_task.uuid, "https://example.com/foo", None).unwrap();

    let flags = link_flags_by_task(&conn).unwrap();

    let pr_flags = flags[&pr_task.uuid.to_string()];
    assert!(pr_flags.any && pr_flags.pr && !pr_flags.issue);

    let issue_flags = flags[&issue_task.uuid.to_string()];
    assert!(issue_flags.any && issue_flags.issue && !issue_flags.pr);

    let generic_flags = flags[&generic_task.uuid.to_string()];
    assert!(generic_flags.any && !generic_flags.pr && !generic_flags.issue);
}

#[test]
fn parse_issue_link_extracts_owner_repo_and_number() {
    assert_eq!(
        parse_issue_link("https://github.com/acme/widgets/issues/7"),
        Some(("acme/widgets".to_string(), 7))
    );
    assert_eq!(
        parse_issue_link("https://github.com/acme/widgets/pull/42"),
        None
    );
    assert_eq!(parse_issue_link("https://example.com/foo"), None);
}

#[test]
fn group_tasks_by_issue_groups_shared_issues_and_buckets_the_rest() {
    let conn = mem();
    let t1 = seed_task(&conn);
    let t2 = seed_task(&conn);
    let t3 = seed_task(&conn);
    let unlinked = seed_task(&conn);

    add_link(
        &conn,
        &t1.uuid,
        "https://github.com/acme/widgets/issues/7",
        None,
    )
    .unwrap();
    add_link(
        &conn,
        &t2.uuid,
        "https://github.com/acme/widgets/issues/7",
        None,
    )
    .unwrap();
    add_link(
        &conn,
        &t3.uuid,
        "https://github.com/acme/widgets/issues/9",
        None,
    )
    .unwrap();

    let tasks = vec![t1.clone(), t2.clone(), t3.clone(), unlinked.clone()];
    let (groups, ungrouped) = group_tasks_by_issue(&conn, &tasks).unwrap();

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].owner_repo, "acme/widgets");
    assert_eq!(groups[0].number, 7);
    assert_eq!(
        groups[0].tasks.iter().map(|t| t.uuid).collect::<Vec<_>>(),
        vec![t1.uuid, t2.uuid]
    );
    assert_eq!(groups[1].number, 9);
    assert_eq!(
        groups[1].tasks.iter().map(|t| t.uuid).collect::<Vec<_>>(),
        vec![t3.uuid]
    );

    assert_eq!(ungrouped.len(), 1);
    assert_eq!(ungrouped[0].uuid, unlinked.uuid);
}

#[test]
fn is_url_detects_links_vs_paths() {
    assert!(is_url("https://github.com/a/b/pull/1"));
    assert!(is_url("http://example.com"));
    assert!(is_url("www.test.dk"));
    assert!(!is_url("src/main.rs"));
    assert!(!is_url("Cargo.toml"));
}

#[test]
fn add_and_get_links_with_history() {
    let conn = mem();
    let task = seed_task(&conn);
    add_link(
        &conn,
        &task.uuid,
        "https://github.com/acme/widgets/pull/42",
        None,
    )
    .unwrap();
    let links = get_links(&conn, &task.uuid).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].display(), "PR #42 · acme/widgets");

    let history = get_history(&conn, &task.uuid).unwrap();
    assert!(
        history
            .iter()
            .any(|h| h.field == "link" && h.new_value.as_deref() == Some("PR #42 · acme/widgets"))
    );
}

#[test]
fn delete_link_records_removal_history() {
    let conn = mem();
    let task = seed_task(&conn);
    add_link(&conn, &task.uuid, "https://example.com/x", Some("My link")).unwrap();
    let links = get_links(&conn, &task.uuid).unwrap();
    assert!(delete_link(&conn, links[0].id).unwrap());
    assert!(get_links(&conn, &task.uuid).unwrap().is_empty());

    let history = get_history(&conn, &task.uuid).unwrap();
    assert!(
        history
            .iter()
            .any(|h| h.field == "link" && h.old_value.as_deref() == Some("My link"))
    );
}
