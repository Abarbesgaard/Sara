use super::*;
use crate::infrastructure::model::Item;

fn seed(conn: &Connection) -> Item {
    let mut item = Item::new_memory(
        "old title".to_string(),
        "old body about frobnicators".to_string(),
        None,
    );
    item.tags = vec!["oldtag".to_string()];
    item.path = Some(String::new());
    db::insert_item(conn, &mut item).unwrap();
    item
}

#[test]
fn relearn_replaces_body_and_reindexes_fts() {
    let conn = db::open_in_memory_for_test();
    let item = seed(&conn);
    let label = format!("m{}", item.display_id.unwrap());

    relearn_value(
        &conn,
        &label,
        Some("new body about widgets"),
        &[],
        &[],
        false,
    )
    .unwrap();

    let loaded = db::get_item_by_uuid(&conn, &item.uuid.to_string()).unwrap();
    assert_eq!(loaded.body, "new body about widgets");
    assert_eq!(loaded.title, "new body about widgets");
    assert_eq!(db::search_fts(&conn, "widgets", 10).unwrap().len(), 1);
    assert!(
        db::search_fts(&conn, "frobnicators", 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn relearn_replaces_tags_and_preserves_body_and_created() {
    let conn = db::open_in_memory_for_test();
    let item = seed(&conn);
    let label = format!("m{}", item.display_id.unwrap());

    relearn_value(&conn, &label, None, &["newtag".to_string()], &[], false).unwrap();

    let loaded = db::get_item_by_uuid(&conn, &item.uuid.to_string()).unwrap();
    assert_eq!(loaded.body, "old body about frobnicators");
    assert_eq!(loaded.tags, vec!["newtag".to_string()]);
    assert_eq!(loaded.created, item.created);
    assert_eq!(db::find_items_by_tag(&conn, "newtag").unwrap().len(), 1);
    assert!(db::find_items_by_tag(&conn, "oldtag").unwrap().is_empty());
}

#[test]
fn relearn_requires_at_least_one_change() {
    let conn = db::open_in_memory_for_test();
    let item = seed(&conn);
    let label = format!("m{}", item.display_id.unwrap());

    assert!(relearn_value(&conn, &label, None, &[], &[], false).is_err());
}

#[test]
fn relearn_refreshes_a_stale_semantic_embedding() {
    use crate::infrastructure::memory::embedding;

    let conn = db::open_in_memory_for_test();
    let item = seed(&conn);
    let label = format!("m{}", item.display_id.unwrap());

    embedding::index_memory(&conn, &item);
    let before = db::get_embedding(&conn, &item.uuid.to_string())
        .unwrap()
        .expect("memory should be indexed");

    relearn_value(
        &conn,
        &label,
        Some("kubernetes pod eviction under memory pressure"),
        &[],
        &[],
        false,
    )
    .unwrap();

    let after = db::get_embedding(&conn, &item.uuid.to_string())
        .unwrap()
        .expect("embedding must still exist after relearn");
    assert_ne!(
        before, after,
        "relearn must refresh the embedding so semantic recall matches the new body"
    );
    let loaded = db::get_item_by_uuid(&conn, &item.uuid.to_string()).unwrap();
    let recomputed = {
        embedding::index_memory(&conn, &loaded);
        db::get_embedding(&conn, &item.uuid.to_string())
            .unwrap()
            .unwrap()
    };
    assert_eq!(
        after, recomputed,
        "embedding must reflect the new body text"
    );
}

#[test]
fn relearn_does_not_index_an_unembedded_memory() {
    let conn = db::open_in_memory_for_test();
    let item = seed(&conn);
    let label = format!("m{}", item.display_id.unwrap());

    relearn_value(&conn, &label, Some("a brand new body"), &[], &[], false).unwrap();
    assert!(
        db::get_embedding(&conn, &item.uuid.to_string())
            .unwrap()
            .is_none(),
        "relearn must not create an embedding for a memory that had none"
    );
}

#[test]
fn relearn_enforces_safety_guardrails_on_new_body() {
    let conn = db::open_in_memory_for_test();
    let item = seed(&conn);
    let label = format!("m{}", item.display_id.unwrap());

    assert!(relearn_value(&conn, &label, Some("api_key=verysecret"), &[], &[], false).is_err());
    assert!(relearn_value(&conn, &label, Some("api_key=verysecret"), &[], &[], true).is_ok());
}

#[test]
fn relearn_stores_file_paths_as_absolute() {
    let conn = db::open_in_memory_for_test();
    let item = seed(&conn);
    let label = format!("m{}", item.display_id.unwrap());

    relearn_value(
        &conn,
        &label,
        None,
        &[],
        &["src/widget.rs".to_string()],
        true,
    )
    .unwrap();

    let stored = db::get_item_files(&conn, &item.uuid).unwrap();
    assert_eq!(stored.len(), 1);
    assert!(
        std::path::Path::new(&stored[0]).is_absolute(),
        "relearn stored a non-absolute path: {}",
        stored[0]
    );
    assert!(
        stored[0].ends_with("src/widget.rs"),
        "unexpected path: {}",
        stored[0]
    );
}
