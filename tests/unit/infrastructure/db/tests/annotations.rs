use super::*;

#[test]
fn adding_annotation_records_a_history_event() {
    let conn = mem();
    let task = seed_task(&conn);
    add_annotation(&conn, &task.uuid, "This is a test comment").unwrap();

    let history = get_history(&conn, &task.uuid).unwrap();
    let ann: Vec<_> = history.iter().filter(|h| h.field == "annotation").collect();
    assert_eq!(ann.len(), 1);
    assert_eq!(ann[0].new_value.as_deref(), Some("This is a test comment"));
    assert!(ann[0].old_value.is_none());
}

#[test]
fn deleting_annotation_records_a_removal_event() {
    let conn = mem();
    let task = seed_task(&conn);
    add_annotation(&conn, &task.uuid, "temp note").unwrap();
    let anns = get_annotations(&conn, &task.uuid).unwrap();
    assert_eq!(anns.len(), 1);

    delete_annotation(&conn, anns[0].id).unwrap();

    let history = get_history(&conn, &task.uuid).unwrap();
    let removals: Vec<_> = history
        .iter()
        .filter(|h| h.field == "annotation" && h.new_value.is_none())
        .collect();
    assert_eq!(removals.len(), 1);
    assert_eq!(removals[0].old_value.as_deref(), Some("temp note"));
}
