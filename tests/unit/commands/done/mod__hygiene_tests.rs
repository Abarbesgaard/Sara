use super::*;
use crate::infrastructure::model::Item;

fn mem(conn: &Connection, title: &str) -> Item {
    crate::test_support::seed_memory(conn, title, &format!("body of {title}"), &[])
}

fn label(item: &Item) -> String {
    format!("m{}", item.display_id.unwrap_or(0))
}

#[test]
fn done_auto_archives_superseded() {
    let conn = db::open_in_memory_for_test();
    let old = mem(&conn, "old finding");
    let new = mem(&conn, "new finding replaces old");
    db::insert_memory_link(
        &conn,
        &new.uuid.to_string(),
        &old.uuid.to_string(),
        "supersedes",
        1.0,
    )
    .unwrap();

    let task = crate::test_support::seed_task(&conn, "hygiene demo", "proj");
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

    assert!(
        db::get_item_by_handle(&conn, &label(&old)).is_err(),
        "old memory is no longer active"
    );
    assert!(
        db::get_item_by_handle(&conn, &label(&new)).is_ok(),
        "superseding memory stays active"
    );
}
