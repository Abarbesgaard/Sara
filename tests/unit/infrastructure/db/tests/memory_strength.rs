use super::*;
use crate::infrastructure::model::{Item, Status};
use chrono::Utc;
use uuid::Uuid;

#[test]
fn memory_recall_daily_counts_batch_matches_per_item() {
    let conn = mem();
    let mut hot = make_memory("hot", &[]);
    insert_item(&conn, &mut hot).unwrap();
    let mut warm = make_memory("warm", &[]);
    insert_item(&conn, &mut warm).unwrap();
    let mut cold = make_memory("cold", &[]);
    insert_item(&conn, &mut cold).unwrap();

    for _ in 0..5 {
        record_memory_recall(&conn, &hot.uuid).unwrap();
    }
    record_memory_recall(&conn, &warm.uuid).unwrap();

    for (days_ago, target) in [(2_i64, &hot), (6, &warm), (9, &hot), (-3, &warm)] {
        let at = (Utc::now() - chrono::Duration::days(days_ago)).to_rfc3339();
        conn.execute(
            "INSERT INTO events (action, ref_uuid, kind, tags_json, project, at)
                 VALUES ('memory_recalled', ?1, 'memory', '[]', NULL, ?2)",
            rusqlite::params![target.uuid.to_string(), at],
        )
        .unwrap();
    }

    let batch = memory_recall_daily_counts_all(&conn, 7);
    for item in [&hot, &warm, &cold] {
        let per_item = memory_recall_daily_counts(&conn, &item.uuid, 7);
        let batched = batch
            .get(&item.uuid)
            .cloned()
            .unwrap_or_else(|| vec![0u64; 7]);
        assert_eq!(
            per_item, batched,
            "{} disagreed: per-item {per_item:?} vs batched {batched:?}",
            item.title
        );
    }

    let hot_counts = batch.get(&hot.uuid).cloned().unwrap_or_default();
    assert!(
        hot_counts.iter().filter(|c| **c > 0).count() >= 2,
        "fixture too flat: {hot_counts:?}"
    );
    assert!(!batch.contains_key(&cold.uuid), "no recalls -> absent");
}

#[test]
fn item_strengths_batch_matches_item_strength() {
    let conn = mem();

    let mut plain = make_memory("plain", &[]);
    insert_item(&conn, &mut plain).unwrap();

    let mut recalled = make_memory("recalled", &[]);
    insert_item(&conn, &mut recalled).unwrap();
    for _ in 0..4 {
        record_memory_recall(&conn, &recalled.uuid).unwrap();
    }

    let mut canonical = make_memory("canonical", &[]);
    insert_item(&conn, &mut canonical).unwrap();
    for i in 0..7 {
        let mut child = make_memory(&format!("child {i}"), &[]);
        insert_item(&conn, &mut child).unwrap();
        insert_memory_link(
            &conn,
            &child.uuid.to_string(),
            &canonical.uuid.to_string(),
            "derived_from",
            1.0,
        )
        .unwrap();
    }

    let mut both = make_memory("both", &[]);
    insert_item(&conn, &mut both).unwrap();
    record_memory_recall(&conn, &both.uuid).unwrap();
    let mut both_child = make_memory("both child", &[]);
    insert_item(&conn, &mut both_child).unwrap();
    insert_memory_link(
        &conn,
        &both_child.uuid.to_string(),
        &both.uuid.to_string(),
        "derived_from",
        1.0,
    )
    .unwrap();

    let mut task = seed_task(&conn);
    task.status = Status::Completed;
    update_task(&conn, &task).unwrap();
    let mut prov = Item::new_memory("provisional".to_string(), "b".to_string(), Some(task.uuid));
    prov.path = Some(String::new());
    prov.status = "provisional".to_string();
    insert_item(&conn, &mut prov).unwrap();

    let mut promoted = Item::new_memory("promoted".to_string(), "b".to_string(), Some(task.uuid));
    promoted.path = Some(String::new());
    insert_item(&conn, &mut promoted).unwrap();

    let items = list_memories(&conn).unwrap();
    assert!(items.len() >= 4);
    let batch = item_strengths(&conn, &items);

    for item in &items {
        let per_item = item_strength(&conn, item);
        let batched = batch.get(&item.uuid).copied().unwrap_or(f64::NAN);
        assert!(
            (per_item - batched).abs() < 1e-9,
            "{}: per-item {per_item} != batched {batched}",
            item.title
        );
    }

    let mut distinct: Vec<String> = batch.values().map(|v| format!("{v:.4}")).collect();
    distinct.sort();
    distinct.dedup();
    assert!(
        distinct.len() >= 3,
        "fixture too flat to be meaningful: {distinct:?}"
    );
}

#[test]
fn recall_usage_boosts_item_strength_within_window() {
    let conn = mem();
    let mut item = make_memory("used often", &[]);
    insert_item(&conn, &mut item).unwrap();
    assert_eq!(item_strength(&conn, &item), 1.0);

    for _ in 0..3 {
        record_memory_recall(&conn, &item.uuid).unwrap();
    }
    let s = item_strength(&conn, &item);
    assert!((s - 1.3).abs() < 1e-9, "expected 1.3, got {s}");

    for _ in 0..20 {
        record_memory_recall(&conn, &item.uuid).unwrap();
    }
    let s = item_strength(&conn, &item);
    assert!((s - 1.5).abs() < 1e-9, "expected cap at 1.5, got {s}");
}

#[test]
fn recall_usage_boost_ignores_events_outside_window() {
    let conn = mem();
    let mut item = make_memory("stale usage", &[]);
    insert_item(&conn, &mut item).unwrap();

    let old = (Utc::now() - chrono::Duration::days(RECALL_BOOST_WINDOW_DAYS + 10)).to_rfc3339();
    conn.execute(
        "INSERT INTO events (action, ref_uuid, kind, tags_json, project, at)
             VALUES ('memory_recalled', ?1, 'memory', '[]', NULL, ?2)",
        rusqlite::params![item.uuid.to_string(), old],
    )
    .unwrap();

    assert_eq!(item_strength(&conn, &item), 1.0);
}

#[test]
fn recall_usage_boosts_batch_matches_per_item_strength() {
    let conn = mem();
    let mut a = make_memory("recalled thrice", &[]);
    insert_item(&conn, &mut a).unwrap();
    let mut b = make_memory("recalled once", &[]);
    insert_item(&conn, &mut b).unwrap();
    let mut c = make_memory("never recalled", &[]);
    insert_item(&conn, &mut c).unwrap();

    for _ in 0..3 {
        record_memory_recall(&conn, &a.uuid).unwrap();
    }
    record_memory_recall(&conn, &b.uuid).unwrap();

    let boosts = recall_usage_boosts(&conn);
    assert!((boosts.get(&a.uuid).copied().unwrap_or(0.0) - 0.3).abs() < 1e-9);
    assert!((boosts.get(&b.uuid).copied().unwrap_or(0.0) - 0.1).abs() < 1e-9);
    assert!(!boosts.contains_key(&c.uuid), "no events → absent from map");

    for item in [&a, &b, &c] {
        let via_batch =
            item_strength_with_boost(&conn, item, boosts.get(&item.uuid).copied().unwrap_or(0.0));
        assert!((via_batch - item_strength(&conn, item)).abs() < 1e-9);
    }
}

#[test]
fn item_strength_is_baseline_with_no_source_task() {
    let conn = mem();
    let item = make_memory("standalone", &[]);
    assert_eq!(item_strength(&conn, &item), 1.0);
}

#[test]
fn item_strength_is_boosted_for_a_completed_source_task() {
    let conn = mem();
    let mut task = seed_task(&conn);
    task.status = Status::Completed;
    update_task(&conn, &task).unwrap();
    let mut item = Item::new_memory("m".to_string(), "b".to_string(), Some(task.uuid));
    item.path = Some(String::new());

    assert_eq!(item_strength(&conn, &item), 2.0);
}

#[test]
fn provisional_memory_is_not_labeled_strong_despite_completed_source() {
    let conn = mem();
    let mut task = seed_task(&conn);
    task.status = Status::Completed;
    update_task(&conn, &task).unwrap();
    let mut item = Item::new_memory("m".to_string(), "b".to_string(), Some(task.uuid));
    item.path = Some(String::new());
    item.status = "provisional".to_string();

    let s = item_strength(&conn, &item);
    assert!(
        s < 1.5,
        "provisional must not reach Strong/Linked from base: {s}"
    );
    assert_eq!(s, 1.0, "capped to the Weak base band");

    item.status = "active".to_string();
    assert_eq!(item_strength(&conn, &item), 2.0);
}

#[test]
fn item_strength_is_moderately_boosted_for_a_pending_source_task() {
    let conn = mem();
    let task = seed_task(&conn);
    let mut item = Item::new_memory("m".to_string(), "b".to_string(), Some(task.uuid));
    item.path = Some(String::new());

    assert_eq!(item_strength(&conn, &item), 1.5);
}

#[test]
fn item_strength_falls_back_to_baseline_when_source_task_is_gone() {
    let conn = mem();
    let mut item = Item::new_memory("m".to_string(), "b".to_string(), Some(Uuid::new_v4()));
    item.path = Some(String::new());

    assert_eq!(item_strength(&conn, &item), 1.0);
}

#[test]
fn item_strength_uses_item_task_links_when_no_source_task() {
    let conn = mem();
    let mut task = seed_task(&conn);
    task.status = Status::Completed;
    update_task(&conn, &task).unwrap();

    let mut item = Item::new_memory("m".to_string(), "b".to_string(), None);
    item.path = Some(String::new());
    insert_item(&conn, &mut item).unwrap();
    set_item_task_links(&conn, &item.uuid, &[(task.uuid, "auto")]).unwrap();

    assert_eq!(item_strength(&conn, &item), 2.0);
}

#[test]
fn item_strength_is_moderately_boosted_for_pending_linked_task() {
    let conn = mem();
    let task = seed_task(&conn);

    let mut item = Item::new_memory("m".to_string(), "b".to_string(), None);
    item.path = Some(String::new());
    insert_item(&conn, &mut item).unwrap();
    set_item_task_links(&conn, &item.uuid, &[(task.uuid, "auto")]).unwrap();

    assert_eq!(item_strength(&conn, &item), 1.5);
}

#[test]
fn item_base_strengths_batch_matches_per_item() {
    let conn = mem();

    let mut done_task = seed_task(&conn);
    done_task.status = Status::Completed;
    update_task(&conn, &done_task).unwrap();
    let mut m_done = Item::new_memory("done".into(), "b".into(), Some(done_task.uuid));
    m_done.path = Some(String::new());
    insert_item(&conn, &mut m_done).unwrap();

    let pending_task = seed_task(&conn);
    let mut m_pending = Item::new_memory("pending".into(), "b".into(), Some(pending_task.uuid));
    m_pending.path = Some(String::new());
    insert_item(&conn, &mut m_pending).unwrap();

    let mut m_gone = Item::new_memory("gone".into(), "b".into(), Some(Uuid::new_v4()));
    m_gone.path = Some(String::new());
    insert_item(&conn, &mut m_gone).unwrap();

    let mut linked_done = seed_task(&conn);
    linked_done.status = Status::Completed;
    update_task(&conn, &linked_done).unwrap();
    let mut m_linked = Item::new_memory("linked".into(), "b".into(), None);
    m_linked.path = Some(String::new());
    insert_item(&conn, &mut m_linked).unwrap();
    set_item_task_links(&conn, &m_linked.uuid, &[(linked_done.uuid, "auto")]).unwrap();

    let mut m_weak = make_memory("weak", &[]);
    insert_item(&conn, &mut m_weak).unwrap();

    let items = vec![m_done, m_pending, m_gone, m_linked, m_weak];
    let batch = item_base_strengths(&conn, &items);
    for item in &items {
        let expected = item_strength(&conn, item);
        let got = batch.get(&item.uuid).copied().unwrap();
        assert!(
            (got - expected).abs() < 1e-9,
            "batch base strength diverged for {}: got {got}, want {expected}",
            item.uuid
        );
    }
}
