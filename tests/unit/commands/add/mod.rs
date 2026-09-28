use super::*;
use crate::infrastructure::model::Task;

#[test]
fn duplicate_open_task_detected_case_insensitively_same_project() {
    let conn = db::open_in_memory_for_test();
    let mut t = Task::new("Implement JWT login for API".into(), "proj".into());
    db::insert_task(&conn, &mut t).unwrap();

    // Exact-but-differently-cased description in the same project matches.
    let dup = find_duplicate_open_task(&conn, "proj", "  implement jwt login for api ");
    assert!(dup.is_some(), "an open same-project task must be flagged");

    // A distinct description does not match.
    assert!(find_duplicate_open_task(&conn, "proj", "something else").is_none());

    // The same description in a different project does not match.
    assert!(find_duplicate_open_task(&conn, "other", "Implement JWT login for API").is_none());
}

#[test]
fn completed_task_is_not_a_duplicate() {
    use crate::infrastructure::model::Status;
    let conn = db::open_in_memory_for_test();
    let mut t = Task::new("Implement JWT login for API".into(), "proj".into());
    db::insert_task(&conn, &mut t).unwrap();
    t.status = Status::Completed;
    db::update_task(&conn, &t).unwrap();

    // list_tasks only returns pending tasks, so a completed one is not a dup.
    assert!(
        find_duplicate_open_task(&conn, "proj", "Implement JWT login for API").is_none(),
        "a completed task must not block re-adding"
    );
}

#[test]
fn semantic_band_renders_snippet_but_strong_bands_render_full_body() {
    // p2: a long, weak `semantic` hit is truncated to a single snippet line.
    let long: String = "word ".repeat(100); // 500 chars
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

    // A stronger band keeps its full, multi-line body verbatim.
    let full = render_similar_body("canonical", "line1\nline2\nline3");
    assert_eq!(
        full.lines().count(),
        3,
        "a canonical hit prints every line of its body"
    );
    assert!(!full.contains('…'), "a full-body render is never truncated");
}
