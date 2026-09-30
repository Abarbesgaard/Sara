use crate::infrastructure::db;
use crate::infrastructure::memory::embedding;
use crate::infrastructure::model::Item;
use chrono::{Duration, Utc};
use serde_json::Value;
use uuid::Uuid;

fn fresh() -> rusqlite::Connection {
    let conn = db::open_in_memory_for_test();
    embedding::ensure_index_current(&conn).unwrap();
    conn
}

fn memory(conn: &rusqlite::Connection, body: &str, age_days: i64, embed: bool) -> Uuid {
    let mut item = Item::new_memory(body.to_string(), body.to_string(), None);
    item.path = Some(String::new());
    item.created = Utc::now() - Duration::days(age_days);
    db::insert_item(conn, &mut item).unwrap();
    if embed {
        embedding::index_memory(conn, &item);
    }
    item.uuid
}

fn check<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap_or_else(|| panic!("check {id} missing: {v}"))
}

#[test]
fn empty_store_is_healthy_and_reports_every_check() {
    let v = super::doctor_value(&fresh()).unwrap();
    assert_eq!(v["healthy"], true, "{v}");
    let ids: Vec<&str> = v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "embedding_coverage",
            "orphaned_links",
            "duplicates",
            "superseded",
            "provisional_backlog",
            "decay_outliers",
            "stale_anchors"
        ]
    );
    for c in v["checks"].as_array().unwrap() {
        assert_eq!(c["status"], "ok", "{c}");
        assert!(c["fix"].is_null(), "a passing check suggests no fix: {c}");
    }
}

#[test]
fn memory_without_a_vector_warns_with_reindex_fix() {
    let conn = fresh();
    memory(&conn, "retries need jitter", 1, false);
    let v = super::doctor_value(&conn).unwrap();
    let c = check(&v, "embedding_coverage");
    assert_eq!(c["status"], "warn");
    assert_eq!(c["count"], 1);
    assert_eq!(c["fix"], "sara reindex-embeddings");
    assert_eq!(v["healthy"], false);
}

#[test]
fn stale_embedding_scheme_warns() {
    let conn = db::open_in_memory_for_test();
    db::meta_set(&conn, "embedding_scheme_version", "old-model|text0").unwrap();
    let v = super::doctor_value(&conn).unwrap();
    let c = check(&v, "embedding_coverage");
    assert_eq!(c["status"], "warn", "{c}");
    assert_eq!(c["details"]["scheme_current"], false);
}

#[test]
fn link_to_missing_or_archived_memory_is_orphaned() {
    let conn = fresh();
    let a = memory(&conn, "alpha", 1, true);
    let gone = memory(&conn, "beta", 1, true);
    db::insert_memory_link(
        &conn,
        &a.to_string(),
        &Uuid::new_v4().to_string(),
        "similar_to",
        1.0,
    )
    .unwrap();
    db::insert_memory_link(
        &conn,
        &a.to_string(),
        &gone.to_string(),
        "derived_from",
        1.0,
    )
    .unwrap();
    db::archive_item(&conn, &gone).unwrap();

    let v = super::doctor_value(&conn).unwrap();
    let c = check(&v, "orphaned_links");
    assert_eq!(c["status"], "warn");
    assert_eq!(c["count"], 2, "{c}");
}

#[test]
fn archived_target_of_supersedes_is_not_orphaned() {
    let conn = fresh();
    let newer = memory(&conn, "use v2 api", 1, true);
    let older = memory(&conn, "use v1 api", 1, true);
    db::insert_memory_link(
        &conn,
        &newer.to_string(),
        &older.to_string(),
        "supersedes",
        1.0,
    )
    .unwrap();
    db::archive_item(&conn, &older).unwrap();
    let v = super::doctor_value(&conn).unwrap();
    assert_eq!(check(&v, "orphaned_links")["status"], "ok");
}

#[test]
fn near_duplicates_are_flagged_via_diagnose() {
    let conn = fresh();
    let file = "/tmp/sara_doctor_dup.rs".to_string();
    for body in [
        "dependabot bumped NSubstitute to 6.2.0 which broke dotnet restore with NU1608",
        "the dependabot NSubstitute 6.2.0 bump broke the dotnet restore build with NU1608",
    ] {
        let u = memory(&conn, body, 1, true);
        db::set_item_files(&conn, &u, std::slice::from_ref(&file)).unwrap();
    }
    let v = super::doctor_value(&conn).unwrap();
    let c = check(&v, "duplicates");
    assert_eq!(c["status"], "warn", "{c}");
    assert_eq!(c["count"], 1);
    assert_eq!(c["fix"], "sara diagnose-memories");
}

#[test]
fn active_superseded_memory_warns_and_doctor_archives_nothing() {
    let conn = fresh();
    let newer = memory(&conn, "use v2 api", 1, true);
    let older = memory(&conn, "use v1 api", 1, true);
    db::insert_memory_link(
        &conn,
        &newer.to_string(),
        &older.to_string(),
        "supersedes",
        1.0,
    )
    .unwrap();

    let v = super::doctor_value(&conn).unwrap();
    let c = check(&v, "superseded");
    assert_eq!(c["status"], "warn", "{c}");
    assert_eq!(c["fix"], "sara prune-memories --apply");
    assert_eq!(
        db::list_memories(&conn).unwrap().len(),
        2,
        "doctor is read-only"
    );
}

#[test]
fn stale_provisional_memory_is_backlog() {
    let conn = fresh();
    let mut item = Item::new_memory("auto".into(), "auto memory".into(), None);
    item.path = Some(String::new());
    item.status = "provisional".into();
    item.created = Utc::now() - Duration::days(45);
    db::insert_item(&conn, &mut item).unwrap();
    embedding::index_memory(&conn, &item);

    let v = super::doctor_value(&conn).unwrap();
    let c = check(&v, "provisional_backlog");
    assert_eq!(c["status"], "warn", "{c}");
    assert_eq!(c["count"], 1);
}

#[test]
fn unrecalled_old_memory_is_info_and_stays_healthy() {
    let conn = fresh();
    memory(&conn, "ancient lore", 120, true);
    memory(&conn, "forgotten companion", 120, true);
    let recent = memory(&conn, "fresh insight", 1, true);
    db::record_memory_recall(&conn, &recent).unwrap();

    let v = super::doctor_value(&conn).unwrap();
    let c = check(&v, "decay_outliers");
    assert_eq!(c["status"], "info", "{c}");
    assert_eq!(c["details"]["dead_weight"].as_array().unwrap().len(), 2);
    assert_eq!(v["summary"]["warn"], 0, "{v}");
    assert_eq!(v["healthy"], true);
}

#[test]
fn a_memory_recalled_far_above_the_mean_is_over_reinforced() {
    let conn = fresh();
    let hot = memory(&conn, "the one everyone recalls", 1, true);
    for _ in 0..25 {
        db::record_memory_recall(&conn, &hot).unwrap();
    }
    for i in 0..9 {
        let m = memory(&conn, &format!("occasional note {i}"), 1, true);
        db::record_memory_recall(&conn, &m).unwrap();
    }
    let v = super::doctor_value(&conn).unwrap();
    let over = &check(&v, "decay_outliers")["details"]["over_reinforced"];
    assert_eq!(over.as_array().unwrap().len(), 1, "{over}");
    assert_eq!(over[0]["recalls"], 25);
}

#[test]
fn stale_anchors_lists_drifted_and_missing_files_as_info() {
    let conn = fresh();
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, seed: &str| {
        let p = dir.path().join(name);
        let body: String = (0..30).map(|i| format!("{seed}_{i}();\n")).collect();
        std::fs::write(&p, body).unwrap();
        p.to_str().unwrap().to_string()
    };
    let kept = write("kept.rs", "keep");
    let drifted = write("drifted.rs", "old");
    let removed = write("removed.rs", "bye");

    let fresh_mem = memory(&conn, "untouched anchor", 1, true);
    db::set_item_files(&conn, &fresh_mem, std::slice::from_ref(&kept)).unwrap();
    let stale_mem = memory(&conn, "rotting anchors", 1, true);
    db::set_item_files(&conn, &stale_mem, &[drifted.clone(), removed.clone()]).unwrap();

    write("drifted.rs", "new");
    std::fs::remove_file(&removed).unwrap();

    let v = super::doctor_value(&conn).unwrap();
    let c = check(&v, "stale_anchors");
    assert_eq!(c["status"], "info", "{c}");
    assert_eq!(c["count"], 1, "one memory carries the stale anchors: {c}");
    assert!(c["fix"].as_str().unwrap().contains("sara relearn"));
    let details = c["details"].as_array().unwrap();
    assert_eq!(details.len(), 2, "{c}");
    let reason_of = |f: &str| {
        details
            .iter()
            .find(|d| d["file"] == f)
            .map(|d| d["reason"].as_str().unwrap().to_string())
    };
    assert_eq!(reason_of(&drifted).as_deref(), Some("drifted"));
    assert_eq!(reason_of(&removed).as_deref(), Some("missing"));
    assert_eq!(reason_of(&kept), None);
    assert_eq!(v["healthy"], true, "stale anchors are advisory: {v}");
}
