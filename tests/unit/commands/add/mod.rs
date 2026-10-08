use super::*;
use crate::infrastructure::db;

#[test]
fn duplicate_open_task_detected_case_insensitively_same_project() {
    let conn = db::open_in_memory_for_test();
    crate::test_support::seed_task(&conn, "Implement JWT login for API", "proj");

    let dup = find_duplicate_open_task(&conn, "proj", "  implement jwt login for api ");
    assert!(dup.is_some(), "an open same-project task must be flagged");

    assert!(find_duplicate_open_task(&conn, "proj", "something else").is_none());

    assert!(find_duplicate_open_task(&conn, "other", "Implement JWT login for API").is_none());
}

#[test]
fn completed_task_is_not_a_duplicate() {
    use crate::infrastructure::model::Status;
    let conn = db::open_in_memory_for_test();
    let mut t = crate::test_support::seed_task(&conn, "Implement JWT login for API", "proj");
    t.status = Status::Completed;
    db::update_task(&conn, &t).unwrap();

    assert!(
        find_duplicate_open_task(&conn, "proj", "Implement JWT login for API").is_none(),
        "a completed task must not block re-adding"
    );
}

#[test]
fn semantic_band_renders_snippet_but_strong_bands_render_full_body() {
    let long: String = "word ".repeat(100);
    let semantic = render_similar_body("semantic", &long);
    assert_eq!(
        semantic.lines().count(),
        1,
        "a semantic hit collapses to a single snippet line"
    );
    assert!(
        semantic.contains('…'),
        "an over-length semantic body is truncated with an ellipsis"
    );

    let full = render_similar_body("canonical", "line1\nline2\nline3");
    assert_eq!(
        full.lines().count(),
        3,
        "a canonical hit prints every line of its body"
    );
    assert!(!full.contains('…'), "a full-body render is never truncated");
}

fn form(description: &str) -> crate::infrastructure::tui::review_form::FormInput {
    crate::infrastructure::tui::review_form::FormInput {
        description: description.into(),
        project: "proj".into(),
        ..Default::default()
    }
}

#[test]
fn form_with_guide_fields_founds_a_begin_equivalent_task() {
    let conn = db::open_in_memory_for_test();
    let cfg = crate::test_support::cfg();
    let mut f = form("Ship the form");
    f.assignment = "do 4 with the right via".into();
    f.rationale = "parity with begin".into();
    f.acceptance = "form tests pass".into();
    f.verify = "cargo test review_form".into();

    let task = persist::save(&conn, &cfg, f, None, &AddRequest::default()).unwrap();

    let guide = db::get_guide_fields(&conn, &task.uuid).unwrap();
    assert_eq!(guide.assignment.as_deref(), Some("do 4 with the right via"));
    assert_eq!(guide.rationale.as_deref(), Some("parity with begin"));
    let acceptance = db::get_steps(&conn, &task.uuid, db::STEP_KIND_ACCEPTANCE).unwrap();
    assert_eq!(acceptance.len(), 1);
    assert_eq!(acceptance[0].text, "form tests pass");
    assert_eq!(
        acceptance[0].verify_cmd.as_deref(),
        Some("cargo test review_form")
    );
    let steps = db::get_steps(&conn, &task.uuid, db::STEP_KIND_STEP).unwrap();
    assert_eq!(steps.len(), 1, "the recall step is seeded like begin");
    assert_eq!(steps[0].text, "Recall prior art before doing anything else");
}

#[test]
fn guide_assignment_defaults_to_the_title_like_begin() {
    let conn = db::open_in_memory_for_test();
    let cfg = crate::test_support::cfg();
    let mut f = form("Write the docs");
    f.acceptance = "README updated".into();

    let task = persist::save(&conn, &cfg, f, None, &AddRequest::default()).unwrap();

    let guide = db::get_guide_fields(&conn, &task.uuid).unwrap();
    assert_eq!(guide.assignment.as_deref(), Some("Write the docs"));
}

#[test]
fn form_without_guide_fields_stays_a_plain_task() {
    let conn = db::open_in_memory_for_test();
    let cfg = crate::test_support::cfg();

    let task = persist::save(&conn, &cfg, form("Plain"), None, &AddRequest::default()).unwrap();

    let guide = db::get_guide_fields(&conn, &task.uuid).unwrap();
    assert!(guide.assignment.is_none());
    assert!(
        db::get_steps(&conn, &task.uuid, db::STEP_KIND_STEP)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn form_links_and_estimate_persist() {
    let conn = db::open_in_memory_for_test();
    let cfg = crate::test_support::cfg();
    let mut f = form("Linked");
    f.estimate = "1h30m".into();
    f.links = "https://github.com/a/b/pull/1, https://github.com/a/b/issues/2".into();

    let task = persist::save(&conn, &cfg, f, None, &AddRequest::default()).unwrap();

    let stored = db::resolve_task(&conn, &task.uuid.to_string()).unwrap();
    assert_eq!(stored.estimate_mins, Some(90));
    let urls: Vec<String> = db::get_links(&conn, &task.uuid)
        .unwrap()
        .into_iter()
        .map(|l| l.url)
        .collect();
    assert_eq!(
        urls,
        vec![
            "https://github.com/a/b/pull/1".to_string(),
            "https://github.com/a/b/issues/2".to_string()
        ]
    );
}

#[test]
fn similar_hits_map_to_task_and_memory_hints() {
    use crate::infrastructure::tui::review_form::{Hint, HintKind};
    let hits = vec![
        serde_json::json!({"ref_kind": "task_desc", "task": 12, "description": "Login for API"}),
        serde_json::json!({"ref_kind": "memory", "memory": "m864", "title": "Board phase 3"}),
        serde_json::json!({"ref_kind": "task_desc", "task": 0, "description": "Old done task"}),
    ];
    assert_eq!(
        similar::to_hints(&hits),
        vec![
            Hint {
                kind: HintKind::Task,
                label: "12".into(),
                title: "Login for API".into()
            },
            Hint {
                kind: HintKind::Memory,
                label: "m864".into(),
                title: "Board phase 3".into()
            },
            Hint {
                kind: HintKind::Task,
                label: "done".into(),
                title: "Old done task".into()
            },
        ]
    );
}
