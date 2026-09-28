//! Unit tests for db::memory_links.

use super::*;
use crate::infrastructure::model::Item;

#[test]
fn similar_to_does_not_block_a_hierarchical_link_but_real_cycles_do() {
    let conn = mem();
    let mut a = Item::new_memory("a".into(), "a".into(), None);
    a.path = Some(String::new());
    insert_item(&conn, &mut a).unwrap();
    let mut b = Item::new_memory("b".into(), "b".into(), None);
    b.path = Some(String::new());
    insert_item(&conn, &mut b).unwrap();

    // A symmetric association a~b must NOT count as a hierarchical path,
    // so consolidating into derived_from b->a is allowed.
    insert_memory_link(
        &conn,
        &a.uuid.to_string(),
        &b.uuid.to_string(),
        "similar_to",
        0.7,
    )
    .unwrap();
    insert_memory_link(
        &conn,
        &b.uuid.to_string(),
        &a.uuid.to_string(),
        "derived_from",
        0.8,
    )
    .expect("similar_to must not trip the hierarchical cycle guard");

    // But a genuine hierarchical cycle (a->b derived_from already exists) is refused.
    let err = insert_memory_link(
        &conn,
        &a.uuid.to_string(),
        &b.uuid.to_string(),
        "derived_from",
        0.8,
    )
    .unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("cycle"),
        "got: {err}"
    );
}
