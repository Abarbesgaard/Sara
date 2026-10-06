use super::*;
use uuid::Uuid;

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

fn seeded() -> (Connection, Item, Item, Task, Task) {
    let conn = mem();
    let mut a = make_memory("alpha", &[]);
    insert_item(&conn, &mut a).unwrap();
    let mut b = make_memory("beta", &[]);
    insert_item(&conn, &mut b).unwrap();
    let t1 = seed_task(&conn);
    let t2 = seed_task(&conn);
    (conn, a, b, t1, t2)
}

#[test]
fn memory_uses_table_exists_on_fresh_db() {
    assert!(table_exists(&mem(), "memory_uses"));
}

#[test]
fn memory_uses_migration_applies_on_upgraded_db() {
    let mut conn = mem();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    conn.execute_batch("DROP TABLE memory_uses;").unwrap();
    conn.pragma_update(None, "user_version", version - 1)
        .unwrap();

    super::super::migrations::apply_migrations(&mut conn).unwrap();
    assert!(table_exists(&conn, "memory_uses"));

    // A DB already at a high watermark with the table present must not abort (m433).
    conn.pragma_update(None, "user_version", version - 1)
        .unwrap();
    super::super::migrations::apply_migrations(&mut conn).unwrap();
}

#[test]
fn memory_uses_record_is_idempotent_and_keeps_first_at() {
    let (conn, a, _, t1, _) = seeded();
    record_memory_use(&conn, &a.uuid, &t1.uuid, MemoryUseKind::Recalled).unwrap();
    let first = memory_uses_for_task(&conn, &t1.uuid).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    record_memory_use(&conn, &a.uuid, &t1.uuid, MemoryUseKind::Recalled).unwrap();
    let second = memory_uses_for_task(&conn, &t1.uuid).unwrap();

    assert_eq!(first.len(), 1);
    assert_eq!(
        first, second,
        "repeat record must not add a row or move `at`"
    );
}

#[test]
fn memory_uses_distinct_kinds_are_separate_rows() {
    let (conn, a, _, t1, _) = seeded();
    for kind in MemoryUseKind::ALL {
        record_memory_use(&conn, &a.uuid, &t1.uuid, kind).unwrap();
    }
    let kinds: Vec<_> = memory_uses_for_task(&conn, &t1.uuid)
        .unwrap()
        .into_iter()
        .map(|u| u.kind)
        .collect();
    assert_eq!(kinds.len(), 3);
    for kind in MemoryUseKind::ALL {
        assert!(kinds.contains(&kind), "missing {kind:?}");
    }
}

#[test]
fn memory_uses_for_task_and_item_filter_correctly() {
    let (conn, a, b, t1, t2) = seeded();
    record_memory_use(&conn, &a.uuid, &t1.uuid, MemoryUseKind::Surfaced).unwrap();
    record_memory_use(&conn, &b.uuid, &t1.uuid, MemoryUseKind::Cited).unwrap();
    record_memory_use(&conn, &a.uuid, &t2.uuid, MemoryUseKind::Recalled).unwrap();

    let for_t1 = memory_uses_for_task(&conn, &t1.uuid).unwrap();
    assert_eq!(for_t1.len(), 2);
    assert!(for_t1.iter().all(|u| u.task_uuid == t1.uuid));

    let for_a = memory_uses_for_item(&conn, &a.uuid).unwrap();
    assert_eq!(for_a.len(), 2);
    assert!(for_a.iter().all(|u| u.item_uuid == a.uuid));
    assert!(
        memory_uses_for_item(&conn, &Uuid::new_v4())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn memory_use_counts_batch_matches_per_item() {
    let (conn, a, b, t1, t2) = seeded();
    let mut idle = make_memory("idle", &[]);
    insert_item(&conn, &mut idle).unwrap();

    record_memory_use(&conn, &a.uuid, &t1.uuid, MemoryUseKind::Surfaced).unwrap();
    record_memory_use(&conn, &a.uuid, &t2.uuid, MemoryUseKind::Surfaced).unwrap();
    record_memory_use(&conn, &a.uuid, &t1.uuid, MemoryUseKind::Cited).unwrap();
    record_memory_use(&conn, &b.uuid, &t2.uuid, MemoryUseKind::Recalled).unwrap();

    let batch = memory_use_counts_all(&conn);
    for item in [&a, &b, &idle] {
        let per_item = memory_use_counts(&conn, &item.uuid);
        let batched = batch.get(&item.uuid).copied().unwrap_or_default();
        assert_eq!(per_item, batched, "{} disagreed", item.title);
    }
    assert_eq!(
        batch[&a.uuid],
        MemoryUseCounts {
            surfaced: 2,
            recalled: 0,
            cited: 1
        }
    );
    assert!(!batch.contains_key(&idle.uuid), "no uses -> absent");
}

#[test]
fn memory_uses_cascade_on_item_and_task_delete() {
    let (conn, a, b, t1, t2) = seeded();
    record_memory_use(&conn, &a.uuid, &t1.uuid, MemoryUseKind::Cited).unwrap();
    record_memory_use(&conn, &b.uuid, &t2.uuid, MemoryUseKind::Cited).unwrap();

    conn.execute("DELETE FROM items WHERE uuid=?1", [a.uuid.to_string()])
        .unwrap();
    assert!(memory_uses_for_task(&conn, &t1.uuid).unwrap().is_empty());

    conn.execute("DELETE FROM tasks WHERE uuid=?1", [t2.uuid.to_string()])
        .unwrap();
    assert!(memory_uses_for_item(&conn, &b.uuid).unwrap().is_empty());
}

#[test]
fn memory_uses_reject_unknown_task() {
    let (conn, a, _, _, _) = seeded();
    assert!(
        record_memory_use(&conn, &a.uuid, &Uuid::new_v4(), MemoryUseKind::Cited).is_err(),
        "FK to tasks must refuse an unknown task"
    );
}

#[test]
fn memory_use_kind_round_trips() {
    for kind in MemoryUseKind::ALL {
        assert_eq!(MemoryUseKind::parse(kind.as_str()), Some(kind));
    }
    assert_eq!(MemoryUseKind::parse("used"), None);
}

fn set_status(conn: &Connection, task: &mut Task, status: Status) {
    task.status = status;
    update_task(conn, task).unwrap();
}

/// a: cited by a verified-completed task, cited by a completed-unverified
/// task, recalled+surfaced but not cited by a completed task (ignored),
/// surfaced by a pending task, and cited by a deleted task (excluded).
/// b: recalled by a pending task only.
fn provenance_fixture() -> (Connection, Item, Item) {
    use MemoryUseKind::*;
    let (conn, a, b, mut verified, mut unverified) = seeded();
    let mut ignoring = seed_task(&conn);
    let pending = seed_task(&conn);
    let mut deleted = seed_task(&conn);
    set_status(&conn, &mut verified, Status::Completed);
    set_validated(&conn, &verified.uuid, "abc1234").unwrap();
    set_status(&conn, &mut unverified, Status::Completed);
    set_status(&conn, &mut ignoring, Status::Completed);
    set_status(&conn, &mut deleted, Status::Deleted);

    for (task, kind) in [
        (&verified, Cited),
        (&verified, Recalled),
        (&unverified, Cited),
        (&ignoring, Recalled),
        (&ignoring, Surfaced),
        (&pending, Surfaced),
        (&deleted, Cited),
    ] {
        record_memory_use(&conn, &a.uuid, &task.uuid, kind).unwrap();
    }
    record_memory_use(&conn, &b.uuid, &pending.uuid, Recalled).unwrap();
    (conn, a, b)
}

#[test]
fn provenance_counts_verified_unverified_and_ignored() {
    let (conn, a, b) = provenance_fixture();

    assert_eq!(
        memory_provenance(&conn, &a.uuid),
        MemoryProvenance {
            cited_verified: 1,
            cited: 2,
            recalled_in_tasks: 2,
            surfaced_in_tasks: 2,
            ignored: 1,
        }
    );
    assert_eq!(
        memory_provenance(&conn, &b.uuid),
        MemoryProvenance {
            recalled_in_tasks: 1,
            ..Default::default()
        },
        "a pending task's recall is not yet ignored"
    );
}

#[test]
fn provenance_batched_twin_matches_per_item() {
    let (conn, a, b) = provenance_fixture();
    let mut unused = make_memory("gamma", &[]);
    insert_item(&conn, &mut unused).unwrap();

    let all = memory_provenance_all(&conn);
    for item in [&a, &b, &unused] {
        assert_eq!(
            all.get(&item.uuid).copied().unwrap_or_default(),
            memory_provenance(&conn, &item.uuid),
            "twin drift for {}",
            item.title
        );
    }
    assert!(memory_provenance(&conn, &unused.uuid).is_empty());
}

#[test]
fn provenance_summary_shows_only_nonzero_parts() {
    let (conn, a, b) = provenance_fixture();
    assert_eq!(
        memory_provenance(&conn, &a.uuid).summary(),
        "✓ cited in 1 verified task · cited in 1 task · recalled in 2 · surfaced in 2 · ignored in 1"
    );
    assert_eq!(memory_provenance(&conn, &b.uuid).summary(), "recalled in 1");
    assert_eq!(MemoryProvenance::default().summary(), "");
}

fn days_ago(n: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now() - chrono::Duration::days(n)
}

fn old_memory(conn: &Connection, title: &str, created_days_ago: i64) -> Item {
    let mut m = make_memory(title, &[]);
    m.created = days_ago(created_days_ago);
    insert_item(conn, &mut m).unwrap();
    m
}

/// A completed task that began 2 days before it ended.
fn finished_task(conn: &Connection, project: &str, ended_days_ago: i64, verified: bool) -> Task {
    let mut t = Task::new("demo".into(), project.into());
    t.entry = days_ago(ended_days_ago + 2);
    insert_task(conn, &mut t).unwrap();
    t.status = Status::Completed;
    t.end = Some(days_ago(ended_days_ago));
    update_task(conn, &t).unwrap();
    if verified {
        set_validated(conn, &t.uuid, "abc1234").unwrap();
    }
    t
}

fn reuse(conn: &Connection, project: Option<&str>) -> ReuseStats {
    knowledge_reuse(conn, project, &days_ago(30)).unwrap()
}

#[test]
fn knowledge_reuse_is_zero_when_no_verified_task_drew_on_memory() {
    let conn = mem();
    old_memory(&conn, "alpha", 100);
    finished_task(&conn, "p", 1, true);
    finished_task(&conn, "p", 1, true);
    assert_eq!(
        reuse(&conn, None),
        ReuseStats {
            verified: 2,
            used_prior: 0,
            cited_prior: 0
        }
    );
}

#[test]
fn knowledge_reuse_counts_cited_and_recalled_but_only_verified_recent_tasks() {
    use MemoryUseKind::*;
    let conn = mem();
    let m = old_memory(&conn, "alpha", 100);
    let cited = finished_task(&conn, "p", 1, true);
    let recalled = finished_task(&conn, "p", 1, true);
    let surfaced_only = finished_task(&conn, "p", 1, true);
    let unverified = finished_task(&conn, "p", 1, false);
    let too_old = finished_task(&conn, "p", 40, true);
    for (t, kind) in [
        (&cited, Cited),
        (&recalled, Recalled),
        (&surfaced_only, Surfaced),
        (&unverified, Cited),
        (&too_old, Cited),
    ] {
        record_memory_use(&conn, &m.uuid, &t.uuid, kind).unwrap();
    }
    assert_eq!(
        reuse(&conn, None),
        ReuseStats {
            verified: 3,
            used_prior: 2,
            cited_prior: 1
        }
    );
}

#[test]
fn knowledge_reuse_is_full_when_every_verified_task_cited() {
    let conn = mem();
    let m = old_memory(&conn, "alpha", 100);
    for _ in 0..3 {
        let t = finished_task(&conn, "p", 1, true);
        record_memory_use(&conn, &m.uuid, &t.uuid, MemoryUseKind::Cited).unwrap();
    }
    assert_eq!(
        reuse(&conn, None),
        ReuseStats {
            verified: 3,
            used_prior: 3,
            cited_prior: 3
        }
    );
}

#[test]
fn knowledge_reuse_ignores_memories_created_after_the_task_began() {
    let conn = mem();
    let t = finished_task(&conn, "p", 1, true);
    let learned_during = old_memory(&conn, "learned during the task", 2);
    record_memory_use(&conn, &learned_during.uuid, &t.uuid, MemoryUseKind::Cited).unwrap();
    assert_eq!(reuse(&conn, None).used_prior, 0);
}

#[test]
fn knowledge_reuse_scopes_to_a_project() {
    let conn = mem();
    let m = old_memory(&conn, "alpha", 100);
    let here = finished_task(&conn, "here", 1, true);
    finished_task(&conn, "elsewhere", 1, true);
    record_memory_use(&conn, &m.uuid, &here.uuid, MemoryUseKind::Recalled).unwrap();
    assert_eq!(
        reuse(&conn, Some("here")),
        ReuseStats {
            verified: 1,
            used_prior: 1,
            cited_prior: 0
        }
    );
    assert_eq!(reuse(&conn, Some("elsewhere")).used_prior, 0);
    assert_eq!(reuse(&conn, None).verified, 2);
}
