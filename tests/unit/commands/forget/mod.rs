use super::*;
use crate::infrastructure::model::Item;

fn seed(conn: &Connection, title: &str, tags: &[&str]) -> Item {
    let mut item = Item::new_memory(title.to_string(), format!("{title} body"), None);
    item.tags = tags.iter().map(|t| t.to_string()).collect();
    item.path = Some(String::new());
    db::insert_item(conn, &mut item).unwrap();
    item
}

fn item_status(conn: &Connection, uuid: &str) -> String {
    db::item_status_for_test(conn, uuid)
}

#[test]
fn forget_plain_memory_has_no_derived_children() {
    let conn = db::open_in_memory_for_test();
    let item = seed(&conn, "lone memory", &[]);
    let label = format!("m{}", item.display_id.unwrap());

    let v = forget_value(&conn, &label, false).unwrap();
    assert!(v["derived"].as_array().unwrap().is_empty());
    assert!(v["cascaded"].as_array().unwrap().is_empty());
    assert_eq!(item_status(&conn, &item.uuid.to_string()), "archived");
}

#[test]
fn forget_canonical_lists_derived_children_but_does_not_archive_them() {
    let conn = db::open_in_memory_for_test();
    let canonical = seed(&conn, "canonical pattern", &[]);
    let child = seed(&conn, "derived application", &[]);
    db::insert_memory_link(
        &conn,
        &child.uuid.to_string(),
        &canonical.uuid.to_string(),
        "derived_from",
        1.0,
    )
    .unwrap();
    let canonical_label = format!("m{}", canonical.display_id.unwrap());
    let child_label = format!("m{}", child.display_id.unwrap());

    let v = forget_value(&conn, &canonical_label, false).unwrap();
    let derived: Vec<&str> = v["derived"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    assert_eq!(derived, vec![child_label.clone()]);
    assert!(v["cascaded"].as_array().unwrap().is_empty());

    assert_eq!(item_status(&conn, &canonical.uuid.to_string()), "archived");
    assert_eq!(item_status(&conn, &child.uuid.to_string()), "active");
}

#[test]
fn forget_cascade_archives_derived_children_too() {
    let conn = db::open_in_memory_for_test();
    let canonical = seed(&conn, "canonical pattern", &[]);
    let child = seed(&conn, "derived application", &[]);
    db::insert_memory_link(
        &conn,
        &child.uuid.to_string(),
        &canonical.uuid.to_string(),
        "derived_from",
        1.0,
    )
    .unwrap();
    let canonical_label = format!("m{}", canonical.display_id.unwrap());
    let child_label = format!("m{}", child.display_id.unwrap());

    let v = forget_value(&conn, &canonical_label, true).unwrap();
    let cascaded: Vec<&str> = v["cascaded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    assert_eq!(cascaded, vec![child_label]);

    assert_eq!(item_status(&conn, &canonical.uuid.to_string()), "archived");
    assert_eq!(item_status(&conn, &child.uuid.to_string()), "archived");
}
