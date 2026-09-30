use super::*;

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
