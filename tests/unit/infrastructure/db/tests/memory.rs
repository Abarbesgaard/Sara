//! Unit tests for db::memory.

use super::*;
use crate::infrastructure::db::*;
use crate::infrastructure::model::{Item, Status, Task};
use uuid::Uuid;

#[test]
fn embedding_upsert_roundtrip() {
    let conn = mem();
    let uuid = Uuid::new_v4().to_string();
    let v = vec![0.1_f32, -0.2, 0.3, 0.4];
    upsert_embedding(&conn, &uuid, &v).unwrap();
    assert_eq!(get_embedding(&conn, &uuid).unwrap(), Some(v.clone()));

    // Upsert replaces (does not duplicate).
    let v2 = vec![1.0_f32, 2.0, 3.0, 4.0];
    upsert_embedding(&conn, &uuid, &v2).unwrap();
    assert_eq!(get_embedding(&conn, &uuid).unwrap(), Some(v2.clone()));

    let all = all_embeddings(&conn).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0], (uuid.clone(), v2));

    delete_embedding(&conn, &uuid).unwrap();
    assert_eq!(get_embedding(&conn, &uuid).unwrap(), None);
    assert!(all_embeddings(&conn).unwrap().is_empty());
}

#[test]
fn active_embeddings_excludes_archived_memories() {
    let conn = mem();
    let mut live = make_memory("live", &[]);
    insert_item(&conn, &mut live).unwrap();
    let mut dead = make_memory("archived", &[]);
    insert_item(&conn, &mut dead).unwrap();

    upsert_embedding(&conn, &live.uuid.to_string(), &[0.1_f32, 0.2, 0.3]).unwrap();
    upsert_embedding(&conn, &dead.uuid.to_string(), &[0.4_f32, 0.5, 0.6]).unwrap();
    archive_item(&conn, &dead.uuid).unwrap();

    // The whole table still holds both vectors...
    assert_eq!(all_embeddings(&conn).unwrap().len(), 2);
    // ...but the active-only scan (what recall ranks) sees only the live one.
    let active = active_embeddings(&conn).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].0, live.uuid.to_string());
}

#[test]
fn find_items_by_file_prefix_escapes_underscore_wildcard() {
    let conn = mem();

    // A memory attached under a directory containing an underscore.
    let mut hit = make_memory("in the underscore dir", &[]);
    insert_item(&conn, &mut hit).unwrap();
    set_item_files(&conn, &hit.uuid, &["/repo/foo_bar/x.rs".into()]).unwrap();

    // A memory under a sibling dir that only matches if `_` is a wildcard.
    let mut miss = make_memory("in the wildcard-collision dir", &[]);
    insert_item(&conn, &mut miss).unwrap();
    set_item_files(&conn, &miss.uuid, &["/repo/fooXbar/y.rs".into()]).unwrap();

    let items = find_items_by_file(&conn, "/repo/foo_bar/", true).unwrap();
    let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
    assert!(
        titles.contains(&"in the underscore dir"),
        "the genuinely-matching path must still be found"
    );
    assert!(
        !titles.contains(&"in the wildcard-collision dir"),
        "an underscore in the query path must not wildcard-match a sibling directory"
    );
}

#[test]
fn insert_item_case_folds_tags_into_item_tags() {
    let conn = mem();
    let mut item = make_memory("m1", &["Service-A", "API"]);
    insert_item(&conn, &mut item).unwrap();

    let counts = list_tags_with_counts(&conn).unwrap();
    let tags: Vec<&str> = counts.iter().map(|(t, _)| t.as_str()).collect();
    assert!(tags.contains(&"service-a"));
    assert!(tags.contains(&"api"));
    assert!(!tags.contains(&"Service-A"));
}

#[test]
fn differently_cased_tags_collide_into_one_vocabulary_entry() {
    let conn = mem();
    let mut a = make_memory("m1", &["service-a"]);
    let mut b = make_memory("m2", &["Service-A"]);
    insert_item(&conn, &mut a).unwrap();
    insert_item(&conn, &mut b).unwrap();

    let counts = list_tags_with_counts(&conn).unwrap();
    assert_eq!(counts, vec![("service-a".to_string(), 2)]);
}

#[test]
fn find_items_by_tag_matches_case_insensitively() {
    let conn = mem();
    let mut item = make_memory("m1", &["Service-A"]);
    insert_item(&conn, &mut item).unwrap();

    let found = find_items_by_tag(&conn, "SERVICE-A").unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].uuid, item.uuid);
}

#[test]
fn set_item_tags_replaces_previous_set() {
    let conn = mem();
    let mut item = make_memory("m1", &["old"]);
    insert_item(&conn, &mut item).unwrap();

    set_item_tags(&conn, &item.uuid, &["new".to_string()]).unwrap();

    assert!(find_items_by_tag(&conn, "old").unwrap().is_empty());
    assert_eq!(find_items_by_tag(&conn, "new").unwrap().len(), 1);
}

#[test]
fn find_items_by_project_matches_exact_case() {
    let conn = mem();
    let mut item = make_memory("m1", &[]);
    insert_item(&conn, &mut item).unwrap();
    set_item_projects(&conn, &item.uuid, &["web-app".to_string()]).unwrap();

    assert_eq!(find_items_by_project(&conn, "web-app").unwrap().len(), 1);
    assert!(find_items_by_project(&conn, "Web-App").unwrap().is_empty());
}

#[test]
fn items_are_indexed_into_search_fts_under_a_namespaced_ref_kind() {
    let conn = mem();
    let mut item = make_memory("service-a auth quirk", &[]);
    item.body = "service-a requires an X-Client-Id header".to_string();
    insert_item(&conn, &mut item).unwrap();

    let hits = search_fts(&conn, "X-Client-Id", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].ref_kind, "item_memory");
    assert_eq!(hits[0].task_uuid, item.uuid.to_string());
}

#[test]
fn provisional_items_are_indexed_into_search_fts() {
    let conn = mem();
    let mut item = make_memory("auto memory", &[]);
    item.body = "frobnicator wiring pattern from done-synthesis".to_string();
    item.status = "provisional".to_string();
    insert_item(&conn, &mut item).unwrap();

    let hits = search_fts(&conn, "done-synthesis", 10).unwrap();
    assert_eq!(hits.len(), 1, "provisional memory must be FTS-searchable");
    assert_eq!(hits[0].ref_kind, "item_memory");
}

#[test]
fn provisional_items_surface_in_tag_and_file_lookups() {
    let conn = mem();
    let mut item = make_memory("auto memory", &["autotag"]);
    item.status = "provisional".to_string();
    insert_item(&conn, &mut item).unwrap();
    set_item_files(&conn, &item.uuid, &["/repo/src/x.rs".to_string()]).unwrap();

    assert_eq!(find_items_by_tag(&conn, "autotag").unwrap().len(), 1);
    assert_eq!(
        find_items_by_file(&conn, "/repo/src/x.rs", false)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn promote_item_activates_only_provisional() {
    let conn = mem();
    let mut item = make_memory("auto memory", &[]);
    item.status = "provisional".to_string();
    insert_item(&conn, &mut item).unwrap();

    assert!(promote_item(&conn, &item.uuid).unwrap());
    let status: String = conn
        .query_row(
            "SELECT status FROM items WHERE uuid=?1",
            [item.uuid.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "active");

    // Second promote is a no-op
    assert!(!promote_item(&conn, &item.uuid).unwrap());
}

#[test]
fn item_ref_kind_does_not_collide_with_annotation_note_ref_kind() {
    let conn = mem();
    let task = seed_named_task(&conn, "unrelated task");
    add_annotation_full(
        &conn,
        &task.uuid,
        "shared-term note",
        "comment",
        "human",
        None,
        None,
        false,
    )
    .unwrap();
    let mut item = make_memory("shared-term memory", &[]);
    item.body = "shared-term".to_string();
    insert_item(&conn, &mut item).unwrap();

    let hits = search_fts(&conn, "shared-term", 10).unwrap();
    let kinds: std::collections::HashSet<&str> = hits.iter().map(|h| h.ref_kind.as_str()).collect();
    assert!(kinds.contains("note"));
    assert!(kinds.contains("item_memory"));
}

#[test]
fn archiving_an_item_removes_it_from_search_fts() {
    let conn = mem();
    let mut item = make_memory("throwaway", &[]);
    item.body = "ephemeral-marker-text".to_string();
    insert_item(&conn, &mut item).unwrap();
    assert_eq!(
        search_fts(&conn, "ephemeral-marker-text", 10)
            .unwrap()
            .len(),
        1
    );

    archive_item(&conn, &item.uuid).unwrap();

    assert!(
        search_fts(&conn, "ephemeral-marker-text", 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn updating_an_active_item_reindexes_its_new_text() {
    let conn = mem();
    let mut item = make_memory("original", &[]);
    item.body = "original-marker-text".to_string();
    insert_item(&conn, &mut item).unwrap();

    item.body = "updated-marker-text".to_string();
    update_item(&conn, &item).unwrap();

    assert!(
        search_fts(&conn, "original-marker-text", 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        search_fts(&conn, "updated-marker-text", 10).unwrap().len(),
        1
    );
}

#[test]
fn source_task_uuid_round_trips_through_insert() {
    let conn = mem();
    let task = seed_task(&conn);
    let mut item = Item::new_memory("m".to_string(), "b".to_string(), Some(task.uuid));
    item.path = Some(String::new());
    insert_item(&conn, &mut item).unwrap();

    let loaded = list_items(&conn, Some("memory")).unwrap();
    assert_eq!(loaded[0].source_task_uuid, Some(task.uuid));
}

#[test]
fn archived_items_are_excluded_from_tag_and_project_lookups() {
    let conn = mem();
    let mut item = make_memory("m1", &["gone"]);
    insert_item(&conn, &mut item).unwrap();
    set_item_projects(&conn, &item.uuid, &["repo".to_string()]).unwrap();

    archive_item(&conn, &item.uuid).unwrap();

    assert!(find_items_by_tag(&conn, "gone").unwrap().is_empty());
    assert!(find_items_by_project(&conn, "repo").unwrap().is_empty());
    assert!(list_tags_with_counts(&conn).unwrap().is_empty());
}

#[test]
fn find_items_by_file_exact_returns_only_matching_item() {
    let conn = mem();
    let mut a = make_memory("auth memory", &[]);
    insert_item(&conn, &mut a).unwrap();
    set_item_files(&conn, &a.uuid, &["/repo/src/auth.rs".to_string()]).unwrap();

    let mut b = make_memory("other memory", &[]);
    insert_item(&conn, &mut b).unwrap();
    set_item_files(&conn, &b.uuid, &["/repo/src/lib.rs".to_string()]).unwrap();

    let hits = find_items_by_file(&conn, "/repo/src/auth.rs", false).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].uuid, a.uuid);
}

#[test]
fn get_item_by_handle_resolves_display_handle_and_uuid_prefix() {
    let conn = mem();
    let mut item = make_memory("stable-handle memory", &[]);
    item.uuid = uuid::Uuid::parse_str("abcd1234-0000-0000-0000-00000000000a").unwrap();
    insert_item(&conn, &mut item).unwrap();

    // Display handle still works.
    let by_handle = get_item_by_handle(&conn, &format!("m{}", item.display_id.unwrap())).unwrap();
    assert_eq!(by_handle.uuid, item.uuid);

    // Full uuid works.
    let by_uuid = get_item_by_handle(&conn, &item.uuid.to_string()).unwrap();
    assert_eq!(by_uuid.uuid, item.uuid);

    // 8-char uuid prefix (as the MCP link-memory docs advertise) works.
    let by_prefix = get_item_by_handle(&conn, "abcd1234").unwrap();
    assert_eq!(by_prefix.uuid, item.uuid);

    // A non-matching prefix is a clean error.
    assert!(get_item_by_handle(&conn, "ffffffff").is_err());
}

#[test]
fn get_item_by_handle_errors_on_ambiguous_uuid_prefix() {
    let conn = mem();
    let mut a = make_memory("mem a", &[]);
    a.uuid = uuid::Uuid::parse_str("dead0000-0000-0000-0000-00000000000a").unwrap();
    insert_item(&conn, &mut a).unwrap();
    let mut b = make_memory("mem b", &[]);
    b.uuid = uuid::Uuid::parse_str("dead1111-0000-0000-0000-00000000000b").unwrap();
    insert_item(&conn, &mut b).unwrap();

    // "dead" matches both — must error, not silently pick one.
    assert!(
        get_item_by_handle(&conn, "dead").is_err(),
        "ambiguous uuid prefix must error instead of arbitrarily returning one item"
    );
}

#[test]
fn find_items_by_file_prefix_returns_all_under_directory() {
    let conn = mem();
    let mut a = make_memory("auth memory", &[]);
    insert_item(&conn, &mut a).unwrap();
    set_item_files(&conn, &a.uuid, &["/repo/src/auth.rs".to_string()]).unwrap();

    let mut b = make_memory("model memory", &[]);
    insert_item(&conn, &mut b).unwrap();
    set_item_files(&conn, &b.uuid, &["/repo/src/model.rs".to_string()]).unwrap();

    let mut c = make_memory("outside memory", &[]);
    insert_item(&conn, &mut c).unwrap();
    set_item_files(&conn, &c.uuid, &["/repo/tests/integration.rs".to_string()]).unwrap();

    let hits = find_items_by_file(&conn, "/repo/src/", true).unwrap();
    assert_eq!(hits.len(), 2);
    let uuids: Vec<_> = hits.iter().map(|h| h.uuid).collect();
    assert!(uuids.contains(&a.uuid));
    assert!(uuids.contains(&b.uuid));
}

#[test]
fn find_items_by_file_excludes_archived_items() {
    let conn = mem();
    let mut item = make_memory("archived memory", &[]);
    insert_item(&conn, &mut item).unwrap();
    set_item_files(&conn, &item.uuid, &["/repo/src/auth.rs".to_string()]).unwrap();
    archive_item(&conn, &item.uuid).unwrap();

    assert!(
        find_items_by_file(&conn, "/repo/src/auth.rs", false)
            .unwrap()
            .is_empty()
    );
    assert!(
        find_items_by_file(&conn, "/repo/src/", true)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn find_items_by_file_returns_empty_when_no_match() {
    let conn = mem();
    let mut item = make_memory("some memory", &[]);
    insert_item(&conn, &mut item).unwrap();
    set_item_files(&conn, &item.uuid, &["/repo/src/auth.rs".to_string()]).unwrap();

    assert!(
        find_items_by_file(&conn, "/repo/src/other.rs", false)
            .unwrap()
            .is_empty()
    );
    assert!(
        find_items_by_file(&conn, "/repo/tests/", true)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn set_item_files_replaces_previous_set() {
    let conn = mem();
    let mut item = make_memory("m", &[]);
    insert_item(&conn, &mut item).unwrap();
    set_item_files(&conn, &item.uuid, &["/repo/src/old.rs".to_string()]).unwrap();
    set_item_files(&conn, &item.uuid, &["/repo/src/new.rs".to_string()]).unwrap();

    assert!(
        find_items_by_file(&conn, "/repo/src/old.rs", false)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        find_items_by_file(&conn, "/repo/src/new.rs", false)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn set_item_task_links_stores_source_labels_correctly() {
    let conn = mem();
    let mut item = make_memory("m", &[]);
    insert_item(&conn, &mut item).unwrap();

    let auto_task = make_completed_task(&conn, "auto task", "/repo/src/auth.rs");
    let mut explicit_task = Task::new("explicit task".to_string(), "Sara".to_string());
    explicit_task.status = Status::Completed;
    insert_task(&conn, &mut explicit_task).unwrap();

    set_item_task_links(
        &conn,
        &item.uuid,
        &[(auto_task.uuid, "auto"), (explicit_task.uuid, "explicit")],
    )
    .unwrap();

    let links = get_item_task_links(&conn, &item.uuid).unwrap();
    assert_eq!(links.len(), 2);
    let auto_link = links
        .iter()
        .find(|(t, _)| t.uuid == auto_task.uuid)
        .unwrap();
    assert_eq!(auto_link.1, "auto");
    let exp_link = links
        .iter()
        .find(|(t, _)| t.uuid == explicit_task.uuid)
        .unwrap();
    assert_eq!(exp_link.1, "explicit");
}

#[test]
fn get_item_task_links_returns_empty_when_no_links() {
    let conn = mem();
    let mut item = make_memory("m", &[]);
    insert_item(&conn, &mut item).unwrap();

    assert!(get_item_task_links(&conn, &item.uuid).unwrap().is_empty());
}

#[test]
fn set_item_task_links_replaces_previous_set() {
    let conn = mem();
    let mut item = make_memory("m", &[]);
    insert_item(&conn, &mut item).unwrap();
    let t1 = make_completed_task(&conn, "t1", "/repo/a.rs");
    let t2 = make_completed_task(&conn, "t2", "/repo/b.rs");

    set_item_task_links(&conn, &item.uuid, &[(t1.uuid, "auto")]).unwrap();
    set_item_task_links(&conn, &item.uuid, &[(t2.uuid, "explicit")]).unwrap();

    let links = get_item_task_links(&conn, &item.uuid).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].0.uuid, t2.uuid);
}

#[test]
fn set_item_files_rolls_back_when_an_insert_fails() {
    let conn = open_in_memory_for_test();
    let mut item = crate::infrastructure::model::Item::new_memory("t".into(), "body".into(), None);
    item.path = Some(String::new());
    insert_item(&conn, &mut item).unwrap();
    set_item_files(
        &conn,
        &item.uuid,
        &["/a/keep.rs".into(), "/a/also.rs".into()],
    )
    .unwrap();

    // Stand in for a real mid-loop failure (SQLITE_BUSY, disk-full, I/O).
    conn.execute_batch(
        "CREATE TRIGGER boom BEFORE INSERT ON item_files
             WHEN NEW.file_path = '/a/BOOM.rs'
             BEGIN SELECT RAISE(ABORT, 'simulated failure'); END;",
    )
    .unwrap();

    let r = set_item_files(
        &conn,
        &item.uuid,
        &[
            "/a/new.rs".into(),
            "/a/BOOM.rs".into(),
            "/a/third.rs".into(),
        ],
    );
    assert!(r.is_err(), "the failing insert must surface as an error");

    assert_eq!(
        get_item_files(&conn, &item.uuid).unwrap(),
        vec!["/a/also.rs".to_string(), "/a/keep.rs".to_string()],
        "a failed replacement must roll back to the original files, not destroy them"
    );
}

#[test]
fn set_item_tags_rolls_back_when_an_insert_fails() {
    let conn = open_in_memory_for_test();
    let mut item = crate::infrastructure::model::Item::new_memory("t".into(), "body".into(), None);
    item.path = Some(String::new());
    insert_item(&conn, &mut item).unwrap();
    set_item_tags(&conn, &item.uuid, &["keep".into()]).unwrap();

    conn.execute_batch(
        "CREATE TRIGGER boom_tags BEFORE INSERT ON item_tags
             WHEN NEW.tag = 'boom'
             BEGIN SELECT RAISE(ABORT, 'simulated failure'); END;",
    )
    .unwrap();

    assert!(set_item_tags(&conn, &item.uuid, &["new".into(), "boom".into()]).is_err());
    let tags: Vec<String> = conn
        .prepare("SELECT tag FROM item_tags WHERE item_uuid = ?1 ORDER BY tag")
        .unwrap()
        .query_map([item.uuid.to_string()], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(tags, vec!["keep".to_string()]);
}

/// `import` calls `set_task_files_sourced` with an already-open
/// transaction. The atomicity guard must nest (SAVEPOINT), not issue a
/// nested BEGIN, which SQLite rejects.

#[test]
fn set_helpers_work_inside_an_existing_transaction() {
    let mut conn = open_in_memory_for_test();
    let mut item = crate::infrastructure::model::Item::new_memory("t".into(), "body".into(), None);
    item.path = Some(String::new());
    insert_item(&conn, &mut item).unwrap();

    let tx = conn.transaction().unwrap();
    set_item_files(&tx, &item.uuid, &["/a/x.rs".into()])
        .expect("must nest inside an open transaction");
    set_item_tags(&tx, &item.uuid, &["t".into()]).unwrap();
    tx.commit().unwrap();

    assert_eq!(
        get_item_files(&conn, &item.uuid).unwrap(),
        vec!["/a/x.rs".to_string()]
    );
}

/// Same allocation race for memory labels (m1, m2, …): two agents running
/// `sara learn` concurrently must not both be handed `m1`.
#[test]
fn concurrent_memory_inserts_get_distinct_labels() {
    let dir = std::env::temp_dir().join(format!("sara-race-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.db");
    {
        let mut c = Connection::open(&path).unwrap();
        set_pragmas(&c).unwrap();
        super::super::migrations::apply_migrations(&mut c).unwrap();
    }

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let mut handles = vec![];
    for n in 0..8 {
        let path = path.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            let c = Connection::open(&path).unwrap();
            set_pragmas(&c).unwrap();
            let mut item = crate::infrastructure::model::Item::new_memory(
                format!("m{n}"),
                "body".into(),
                None,
            );
            item.path = Some(String::new());
            barrier.wait();
            insert_item(&c, &mut item).map(|_| item.display_id.unwrap())
        }));
    }
    let ids: Vec<i64> = handles
        .into_iter()
        .filter_map(|h| h.join().unwrap().ok())
        .collect();

    let mut uniq = ids.clone();
    uniq.sort_unstable();
    uniq.dedup();
    println!("MEMORY IDS: {ids:?}");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        uniq.len(),
        ids.len(),
        "concurrent learns produced duplicate memory labels: {ids:?}"
    );
}
