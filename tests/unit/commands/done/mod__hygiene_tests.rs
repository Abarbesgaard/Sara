use super::*;
use crate::infrastructure::model::{Item, Task};

/// Create an active memory via db primitives (no cross-slice learn import).
fn mem(conn: &Connection, title: &str) -> Item {
    let mut item = Item::new_memory(title.to_string(), format!("body of {title}"), None);
    item.path = Some(String::new());
    db::insert_item(conn, &mut item).unwrap();
    item
}

fn label(item: &Item) -> String {
    format!("m{}", item.display_id.unwrap_or(0))
}

#[test]
fn done_auto_archives_superseded() {
    let conn = db::open_in_memory_for_test();
    let old = mem(&conn, "old finding");
    let new = mem(&conn, "new finding replaces old");
    // new supersedes old (new is active).
    db::insert_memory_link(
        &conn,
        &new.uuid.to_string(),
        &old.uuid.to_string(),
        "supersedes",
        1.0,
    )
    .unwrap();

    let mut task = Task::new("hygiene demo".into(), "proj".into());
    db::insert_task(&conn, &mut task).unwrap();
    let v = done_value(&conn, &Config::default(), &task.uuid.to_string(), false).unwrap();

    let archived: Vec<&str> = v["hygiene"]["archived"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x.as_str())
        .collect();
    assert!(
        archived.contains(&label(&old).as_str()),
        "superseded old memory is auto-archived"
    );

    // Archived memories drop out of the active read path; the superseder stays.
    assert!(
        db::get_item_by_handle(&conn, &label(&old)).is_err(),
        "old memory is no longer active"
    );
    assert!(
        db::get_item_by_handle(&conn, &label(&new)).is_ok(),
        "superseding memory stays active"
    );
}
