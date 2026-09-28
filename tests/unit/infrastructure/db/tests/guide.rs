//! Unit tests for db::guide.

use super::*;

#[test]
fn add_task_file_upserts_anchor_metadata() {
    let conn = mem();
    let task = seed_task(&conn);
    add_task_file(
        &conn,
        &task.uuid,
        "src/db.rs",
        SOURCE_SUGGESTED,
        Some("initial reason"),
        None,
        None,
        None,
    )
    .unwrap();
    // Same path again: ON CONFLICT updates in place, no duplicate row.
    add_task_file(
        &conn,
        &task.uuid,
        "src/db.rs",
        SOURCE_MANUAL,
        Some("better reason"),
        Some("add_step"),
        Some(10),
        Some(57),
    )
    .unwrap();

    let anchors = get_task_anchors(&conn, &task.uuid).unwrap();
    assert_eq!(anchors.len(), 1);
    let a = &anchors[0];
    assert_eq!(a.source, SOURCE_MANUAL);
    assert_eq!(a.reason.as_deref(), Some("better reason"));
    assert_eq!(a.symbol.as_deref(), Some("add_step"));
    assert_eq!((a.line_start, a.line_end), (Some(10), Some(57)));
    assert_eq!(a.location(), " :: add_step (10-57)");
}

#[test]
fn anchor_location_formats_partial_ranges() {
    let single = Anchor {
        path: "x".into(),
        source: SOURCE_MANUAL.into(),
        reason: None,
        symbol: None,
        line_start: Some(42),
        line_end: None,
    };
    assert_eq!(single.location(), " (L42)");

    let bare = Anchor {
        path: "x".into(),
        source: SOURCE_MANUAL.into(),
        reason: None,
        symbol: Some("foo".into()),
        line_start: None,
        line_end: None,
    };
    assert_eq!(bare.location(), " :: foo");
}

#[test]
fn guide_fields_round_trip() {
    let conn = mem();
    let task = seed_task(&conn);
    set_assignment(&conn, &task.uuid, "the original prompt").unwrap();
    set_rationale(&conn, &task.uuid, "because reasons").unwrap();
    set_validated(&conn, &task.uuid, "deadbeef").unwrap();
    set_meta_json(&conn, &task.uuid, r#"{"k":1}"#).unwrap();

    let g = get_guide_fields(&conn, &task.uuid).unwrap();
    assert_eq!(g.assignment.as_deref(), Some("the original prompt"));
    assert_eq!(g.rationale.as_deref(), Some("because reasons"));
    assert_eq!(g.validated_commit.as_deref(), Some("deadbeef"));
    assert!(g.validated_at.is_some());
    assert_eq!(g.meta_json.as_deref(), Some(r#"{"k":1}"#));
}

#[test]
fn ai_runs_are_recorded_and_returned_in_order() {
    let conn = mem();
    let task = seed_task(&conn);
    let r1 = record_ai_run(
        &conn,
        &task.uuid,
        "enrich",
        Some("opus"),
        Some("azure"),
        Some("prompt"),
        Some("{}"),
        Some(100),
        Some(200),
        Some(300),
    )
    .unwrap();
    let r2 = record_ai_run(
        &conn, &task.uuid, "refine", None, None, None, None, None, None, None,
    )
    .unwrap();
    assert!(r2 > r1);

    let runs = get_ai_runs(&conn, &task.uuid).unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].kind, "enrich");
    assert_eq!(runs[0].model.as_deref(), Some("opus"));
    assert_eq!(runs[0].prompt_tokens, Some(100));
    assert_eq!(runs[0].completion_tokens, Some(200));
    assert_eq!(runs[0].total_tokens, Some(300));
    assert_eq!(runs[1].kind, "refine");
    assert!(runs[1].model.is_none());
    assert!(runs[1].prompt_tokens.is_none());
    assert!(runs[1].total_tokens.is_none());
}

#[test]
fn open_feedback_lists_comments_flagged_first_and_resolves() {
    let conn = mem();
    let task = seed_task(&conn);
    // A plain comment, a flagged comment, and a non-comment note.
    add_annotation_full(
        &conn, &task.uuid, "plain", "comment", "human", None, None, false,
    )
    .unwrap();
    let flagged = add_annotation_full(
        &conn,
        &task.uuid,
        "reconsider this",
        "comment",
        "human",
        Some("step"),
        Some("2"),
        true,
    )
    .unwrap();
    add_annotation_full(
        &conn,
        &task.uuid,
        "a finding",
        "finding",
        "ai",
        None,
        None,
        false,
    )
    .unwrap();

    let open = get_open_feedback(&conn, &task.uuid).unwrap();
    assert_eq!(open.len(), 2, "only open comments count as feedback");
    assert_eq!(
        open[0].text, "reconsider this",
        "flagged feedback sorts first"
    );

    // Resolving links the run and drops it from the open set.
    assert!(resolve_annotation(&conn, flagged, Some(7)).unwrap());
    let open = get_open_feedback(&conn, &task.uuid).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].text, "plain");
}

#[test]
fn search_fts_matches_tasks_notes_and_anchors() {
    let conn = mem();
    let task = seed_named_task(&conn, "implement frobnicator widget");
    add_annotation_full(
        &conn,
        &task.uuid,
        "the frobnicator caches results",
        "finding",
        "ai",
        None,
        None,
        false,
    )
    .unwrap();
    add_task_file(
        &conn,
        &task.uuid,
        "src/frob.rs",
        SOURCE_MANUAL,
        Some("frobnicator lives here"),
        None,
        None,
        None,
    )
    .unwrap();

    let kinds: std::collections::HashSet<String> = search_fts(&conn, "frobnicator", 50)
        .unwrap()
        .into_iter()
        .map(|h| h.ref_kind)
        .collect();
    assert!(kinds.contains("task"));
    assert!(kinds.contains("note"));
    assert!(kinds.contains("anchor"));
}

#[test]
fn search_fts_tolerates_quotes_in_query() {
    let conn = mem();
    let task = seed_named_task(&conn, "handle the \"weird\" input");
    // A query containing a double-quote must not blow up the FTS parser.
    let hits = search_fts(&conn, "\"weird\" input", 10).unwrap();
    assert!(hits.iter().any(|h| h.task_uuid == task.uuid.to_string()));
}

#[test]
fn search_fts_tolerates_hyphenated_service_name_query() {
    let conn = mem();
    let mut item = make_memory("service-a note", &[]);
    item.body = "service-a requires an X-Client-Id header".to_string();
    insert_item(&conn, &mut item).unwrap();

    // A bare hyphenated word must not be parsed as column-filter/NOT syntax.
    let hits = search_fts(&conn, "service-a", 10).unwrap();
    assert_eq!(hits.len(), 1);
}

#[test]
fn search_fts_treats_boolean_keywords_as_literal_text() {
    let conn = mem();
    let mut item = make_memory("config and setup notes", &[]);
    item.body = "config-and-setup guide".to_string();
    insert_item(&conn, &mut item).unwrap();

    // Must not error out or be parsed as an FTS5 boolean expression.
    let hits = search_fts(&conn, "config-and-setup", 10).unwrap();
    assert_eq!(hits.len(), 1);
    let hits = search_fts(&conn, "AND OR NOT", 10).unwrap();
    assert!(hits.is_empty(), "no crash on bare boolean keywords");
}

#[test]
fn search_fts_tolerates_wildcard_and_empty_queries() {
    let conn = mem();
    let mut item = make_memory("wildcard note", &[]);
    item.body = "some content".to_string();
    insert_item(&conn, &mut item).unwrap();

    assert!(search_fts(&conn, "*", 10).is_ok());
    assert!(search_fts(&conn, "", 10).is_ok());
    assert!(search_fts(&conn, "-", 10).is_ok());
}

#[test]
fn dependency_closure_returns_blockers_first() {
    let conn = mem();
    // c depends on b, b depends on a  →  closure of c is [a, b, c].
    let a = seed_named_task(&conn, "a");
    let b = seed_named_task(&conn, "b");
    let c = seed_named_task(&conn, "c");
    add_dependency(&conn, &b.uuid, &a.uuid).unwrap();
    add_dependency(&conn, &c.uuid, &b.uuid).unwrap();

    let closure = dependency_closure(&conn, &c.uuid).unwrap();
    assert_eq!(closure, vec![a.uuid, b.uuid, c.uuid]);
}
