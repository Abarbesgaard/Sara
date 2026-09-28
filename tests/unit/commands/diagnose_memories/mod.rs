use crate::infrastructure::{db, embedding, model::Item};
use uuid::Uuid;

/// Seed a memory with a file link, a tag and a real embedding, so the
/// cosine gate is exercised rather than bypassed by the fail-open path.
fn insert_memory_with_file(conn: &rusqlite::Connection, body: &str, tag: &str, file: &str) -> Uuid {
    let mut item = Item::new_memory(body.to_string(), body.to_string(), None);
    item.tags = vec![tag.to_string()];
    item.path = Some(String::new());
    db::insert_item(conn, &mut item).unwrap();
    db::set_item_projects(conn, &item.uuid, &["test".to_string()]).unwrap();
    db::set_item_files(conn, &item.uuid, &[file.to_string()]).unwrap();
    embedding::index_memory(conn, &item);
    item.uuid
}

const NEAR_A: &str =
    "dependabot bumped NSubstitute to 6.2.0 which broke dotnet restore with NU1608";
const NEAR_B: &str =
    "the dependabot NSubstitute 6.2.0 bump broke the dotnet restore build with NU1608";
const FAR: &str = "sourdough bread proofs best overnight in a cold refrigerator";

#[test]
fn near_duplicate_pair_on_same_file_is_reported() {
    let conn = db::open_in_memory_for_test();
    let file = "/tmp/sara_diag_near.rs".to_string();
    insert_memory_with_file(&conn, NEAR_A, "tag-a", &file);
    insert_memory_with_file(&conn, NEAR_B, "tag-b", &file);

    let v = super::diagnose_value(&conn, super::DEFAULT_CONFLICT_THRESHOLD, None, None).unwrap();
    assert_eq!(
        v["count"].as_u64().unwrap(),
        1,
        "a topically near-identical pair on one file is a real candidate"
    );
    let c = v["conflicts"][0]["cosine"].as_f64().unwrap();
    assert!(
        c >= super::DEFAULT_CONFLICT_THRESHOLD as f64,
        "reported cosine {c} must clear the threshold"
    );
}

#[test]
fn unrelated_pair_on_same_file_is_not_reported() {
    let conn = db::open_in_memory_for_test();
    let file = "/tmp/sara_diag_far.rs".to_string();
    insert_memory_with_file(&conn, NEAR_A, "tag-a", &file);
    insert_memory_with_file(&conn, FAR, "tag-b", &file);

    let v = super::diagnose_value(&conn, super::DEFAULT_CONFLICT_THRESHOLD, None, None).unwrap();
    assert_eq!(
        v["count"].as_u64().unwrap(),
        0,
        "sharing a file is co-occurrence, not conflict — the cosine gate must drop this"
    );
}

#[test]
fn pair_without_embeddings_is_kept_fail_open() {
    let conn = db::open_in_memory_for_test();
    let file = "/tmp/sara_diag_noemb.rs".to_string();
    let a = insert_memory_with_file(&conn, NEAR_A, "tag-a", &file);
    let b = insert_memory_with_file(&conn, FAR, "tag-b", &file);
    // Simulate a lagging index: drop both embedding rows.
    for u in [a, b] {
        db::delete_embedding(&conn, &u.to_string()).unwrap();
    }

    let v = super::diagnose_value(&conn, super::DEFAULT_CONFLICT_THRESHOLD, None, None).unwrap();
    assert_eq!(
        v["count"].as_u64().unwrap(),
        1,
        "an unscorable pair must be kept, never silently hidden"
    );
    assert!(v["conflicts"][0]["cosine"].is_null());
}

#[test]
fn candidates_are_sorted_by_cosine_descending() {
    let conn = db::open_in_memory_for_test();
    let file = "/tmp/sara_diag_sort.rs".to_string();
    insert_memory_with_file(&conn, NEAR_A, "tag-a", &file);
    insert_memory_with_file(&conn, NEAR_B, "tag-b", &file);
    insert_memory_with_file(
        &conn,
        "NSubstitute version pinning is handled in Directory.Packages.props",
        "tag-c",
        &file,
    );

    // Threshold 0 keeps every pair so the ordering itself is under test.
    let v = super::diagnose_value(&conn, 0.0, None, None).unwrap();
    let scores: Vec<f64> = v["conflicts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["cosine"].as_f64().unwrap())
        .collect();
    assert!(scores.len() >= 2, "expected several pairs to order");
    assert!(
        scores.windows(2).all(|w| w[0] >= w[1]),
        "worst offender must come first, got {scores:?}"
    );
}

#[test]
fn limit_truncates_but_total_reports_the_full_count() {
    let conn = db::open_in_memory_for_test();
    let file = "/tmp/sara_diag_limit.rs".to_string();
    insert_memory_with_file(&conn, NEAR_A, "tag-a", &file);
    insert_memory_with_file(&conn, NEAR_B, "tag-b", &file);
    insert_memory_with_file(&conn, FAR, "tag-c", &file);

    let v = super::diagnose_value(&conn, 0.0, None, Some(1)).unwrap();
    assert_eq!(v["count"].as_u64().unwrap(), 1);
    assert!(
        v["total"].as_u64().unwrap() > 1,
        "total must report the untruncated count"
    );
}

#[test]
fn project_scope_excludes_other_provinces() {
    let conn = db::open_in_memory_for_test();
    let file = "/tmp/sara_diag_scope.rs".to_string();
    let a = insert_memory_with_file(&conn, NEAR_A, "tag-a", &file);
    insert_memory_with_file(&conn, NEAR_B, "tag-b", &file);
    // Move one side of the pair into a different project.
    db::set_item_projects(&conn, &a, &["elsewhere".to_string()]).unwrap();

    let scoped =
        super::diagnose_value(&conn, super::DEFAULT_CONFLICT_THRESHOLD, Some("test"), None)
            .unwrap();
    assert_eq!(
        scoped["count"].as_u64().unwrap(),
        0,
        "a pair straddling two projects must not surface in either scope"
    );

    let unscoped =
        super::diagnose_value(&conn, super::DEFAULT_CONFLICT_THRESHOLD, None, None).unwrap();
    assert_eq!(unscoped["count"].as_u64().unwrap(), 1);
}

#[test]
fn linked_memories_do_not_appear_in_diagnose() {
    let conn = db::open_in_memory_for_test();
    let file = "/tmp/sara_diag_linked_test.rs".to_string();
    let uuid_a = insert_memory_with_file(&conn, NEAR_A, "tag-c", &file);
    let uuid_b = insert_memory_with_file(&conn, NEAR_B, "tag-d", &file);

    // Link them — they should disappear from diagnose output.
    db::insert_memory_link(
        &conn,
        &uuid_a.to_string(),
        &uuid_b.to_string(),
        "supersedes",
        1.0,
    )
    .unwrap();

    let v = super::diagnose_value(&conn, super::DEFAULT_CONFLICT_THRESHOLD, None, None).unwrap();
    assert_eq!(v["count"].as_u64().unwrap(), 0);
}
