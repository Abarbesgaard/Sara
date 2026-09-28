//! Memory strength scoring: base strength, canonical bonuses, recall-usage boost, daily counts.
//!
//! Part of the db memory subsystem (issue #168); re-exported by `super`.

use super::*;
use crate::infrastructure::model::{Item, Status};
use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

/// Task-linkage-derived confidence signal for ranking a memory in recall: a
/// manually authored note with no source task stays at the baseline. A memory
/// tied to a task is boosted, more so if that task actually completed
/// (evidence the underlying approach worked) than if it's still pending or the
/// source task can no longer be found (loose reference, may have been
/// deleted/reset -- the memory still stands on its own, just without the boost).
///
/// A canonical memory (one with incoming `derived_from` edges) also gets a
/// passive strength boost: +0.1 per derived child, capped at +0.5. A pattern
/// applied across 5+ contexts is evidence of value even if the canonical
/// itself is rarely recalled directly — without this it can still be pruned
pub fn item_strength(conn: &Connection, item: &Item) -> f64 {
    item_base_strength(conn, item)
        + recall_usage_boost(conn, &item.uuid)
        + canonical_derived_bonus(conn, &item.uuid)
}

/// Batched form of [`item_strength`] for scoring a whole set of items at once.
///
/// `item_strength` costs one `COUNT(*)` over `events` **per item**, and that
/// scan is bounded by the number of `memory_recalled` rows rather than by the
/// item — so listing every memory degrades as O(memories × recall events).
/// On a real store (388 memories, ~7k recall events) that was 121 ms of a
/// 138 ms `sara memories`, i.e. ~2.7M row visits to compute 388 numbers.
///
/// Here the two set-wide components are each fetched in a single grouped query
/// and looked up per item, composing the batched forms the graph build already
/// relies on ([`item_base_strengths`], [`recall_usage_boosts`],
/// [`canonical_derived_bonuses`]). The result is identical to calling
/// [`item_strength`] on each item — pinned by
/// `item_strengths_batch_matches_item_strength`.
pub fn item_strengths(conn: &Connection, items: &[Item]) -> std::collections::HashMap<Uuid, f64> {
    let bases = item_base_strengths(conn, items);
    let boosts = recall_usage_boosts(conn);
    let bonuses = canonical_derived_bonuses(conn);
    items
        .iter()
        .map(|item| {
            // `item_base_strengths` returns an entry for every item passed, so
            // the default is unreachable; 1.0 is the Weak floor regardless.
            let s = bases.get(&item.uuid).copied().unwrap_or(1.0)
                + boosts.get(&item.uuid).copied().unwrap_or(0.0)
                + bonuses.get(&item.uuid).copied().unwrap_or(0.0);
            (item.uuid, s)
        })
        .collect()
}

/// +0.1 per incoming `derived_from` edge, capped at +0.5 (see [`item_strength`]).
fn canonical_derived_bonus(conn: &Connection, item_uuid: &Uuid) -> f64 {
    let count = get_memory_links_to(conn, &item_uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .count();
    (count as f64 * 0.1).min(0.5)
}

/// Task-linkage-derived base strength (see module docs on `item_strength`).
fn item_base_strength(conn: &Connection, item: &Item) -> f64 {
    let base = 'base: {
        if let Some(source) = item.source_task_uuid {
            match get_task_by_uuid_prefix(conn, &source.to_string()) {
                Ok(Some(task)) if task.status == Status::Completed => break 'base 2.0,
                Ok(Some(_)) => break 'base 1.5,
                _ => {}
            }
        }
        // Fall back to item_task_links: memories linked via `sara learn --task` /
        // file auto-detection carry real task linkage even without a
        // source_task_uuid, and must not be scored (and pruned) as Weak.
        let linked: Vec<Status> = get_item_task_links(conn, &item.uuid)
            .unwrap_or_default()
            .into_iter()
            .map(|(t, _)| t.status)
            .collect();
        base_strength_from_links(&linked)
    };
    cap_provisional(&item.status, base)
}

/// Base-strength cap for provisional (auto-synthesised, unreviewed) memories.
/// A provisional whose source task is Completed would otherwise score 2.0 =
/// "Strong" the instant `sara done` creates it — 0 recalls, no human review —
/// overstating trust and polluting recall precision before promotion.
const PROVISIONAL_BASE_CAP: f64 = 1.0;

/// Hold a provisional memory's *base* strength in the Weak band so its label
/// reflects that it is a hint until `sara promote` accepts it. Recall-usage and
/// canonical-derived bonuses can still lift a genuinely useful provisional on
/// top of this base; promotion (status -> active) removes the cap entirely.
fn cap_provisional(status: &str, base: f64) -> f64 {
    if status == "provisional" {
        base.min(PROVISIONAL_BASE_CAP)
    } else {
        base
    }
}

/// Shared core of the base-strength fallback: given the statuses of a memory's
/// explicitly-linked tasks, a completed link makes it Strong (2.0), an
/// otherwise-pending link Linked (1.5), and nothing keeps it Weak (1.0). Used
/// by both [`item_base_strength`] and its batched form so they can never drift.
fn base_strength_from_links(linked: &[Status]) -> f64 {
    let mut best = 1.0_f64;
    for st in linked {
        match st {
            Status::Completed => return 2.0,
            Status::Pending => best = 1.5,
            _ => {}
        }
    }
    best
}

/// Batched form of [`item_base_strength`] for scoring many memories at once
/// (the graph build). Two bulk queries — every task's status, and every
/// item→task link's status — replace the 1–2 queries `item_base_strength` runs
/// per memory. The value for each item is identical to `item_base_strength`.
pub fn item_base_strengths(
    conn: &Connection,
    items: &[Item],
) -> std::collections::HashMap<Uuid, f64> {
    // Every task's status (source_task_uuid resolution). A full uuid resolves
    // to exactly one row, matching `get_task_by_uuid_prefix`; deleted tasks are
    // included, matching that helper's status-agnostic lookup.
    let mut task_status: std::collections::HashMap<String, Status> =
        std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT uuid, status FROM tasks")
        && let Ok(rows) =
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
    {
        for (u, s) in rows.flatten() {
            task_status.insert(u, status_from_str(&s));
        }
    }
    // Every item's linked-task statuses (the fallback path), keyed by item uuid.
    let mut link_status: std::collections::HashMap<Uuid, Vec<Status>> =
        std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT l.item_uuid, t.status FROM item_task_links l JOIN tasks t ON t.uuid = l.task_uuid",
    ) && let Ok(rows) =
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
    {
        for (item_uuid, status) in rows.flatten() {
            if let Ok(u) = Uuid::parse_str(&item_uuid) {
                link_status
                    .entry(u)
                    .or_default()
                    .push(status_from_str(&status));
            }
        }
    }

    let mut out = std::collections::HashMap::with_capacity(items.len());
    for item in items {
        let strength = match item
            .source_task_uuid
            .and_then(|s| task_status.get(&s.to_string()))
        {
            Some(Status::Completed) => 2.0,
            Some(_) => 1.5,
            None => base_strength_from_links(link_status.get(&item.uuid).map_or(&[], |v| v)),
        };
        out.insert(item.uuid, cap_provisional(&item.status, strength));
    }
    out
}

/// Batched form of [`canonical_derived_bonus`]: one bulk query over
/// `memory_links` instead of a per-item query, for scoring many memories at
/// once (the graph build). Callers combining this with [`item_base_strengths`]
/// get the same total as [`item_strength`] would compute per item.
pub fn canonical_derived_bonuses(conn: &Connection) -> std::collections::HashMap<Uuid, f64> {
    let mut counts: std::collections::HashMap<Uuid, u32> = std::collections::HashMap::new();
    if let Ok(mut stmt) =
        conn.prepare("SELECT to_uuid FROM memory_links WHERE relation = 'derived_from'")
        && let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0))
    {
        for uuid_str in rows.flatten() {
            if let Ok(u) = Uuid::parse_str(&uuid_str) {
                *counts.entry(u).or_insert(0) += 1;
            }
        }
    }
    counts
        .into_iter()
        .map(|(u, c)| (u, (c as f64 * 0.1).min(0.5)))
        .collect()
}

/// Map a stored task-status string to [`Status`] (mirrors `row_to_task`).
fn status_from_str(s: &str) -> Status {
    match s {
        "completed" => Status::Completed,
        "deleted" => Status::Deleted,
        _ => Status::Pending,
    }
}

/// All memories' file anchors in one query, keyed by item uuid (the batched
/// form of [`get_item_files`] for the graph build).
pub fn all_item_files(conn: &Connection) -> std::collections::HashMap<Uuid, Vec<String>> {
    let mut map: std::collections::HashMap<Uuid, Vec<String>> = std::collections::HashMap::new();
    if let Ok(mut stmt) =
        conn.prepare("SELECT item_uuid, file_path FROM item_files ORDER BY item_uuid, file_path")
        && let Ok(rows) =
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
    {
        for (item_uuid, path) in rows.flatten() {
            if let Ok(u) = Uuid::parse_str(&item_uuid) {
                map.entry(u).or_default().push(path);
            }
        }
    }
    map
}

/// All memories' linked task uuids in one query, keyed by item uuid (the
/// batched form of the task-anchor half of [`get_item_task_links`] for the
/// graph build).
pub fn all_item_task_uuids(conn: &Connection) -> std::collections::HashMap<Uuid, Vec<Uuid>> {
    let mut map: std::collections::HashMap<Uuid, Vec<Uuid>> = std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT item_uuid, task_uuid FROM item_task_links")
        && let Ok(rows) =
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
    {
        for (item_uuid, task_uuid) in rows.flatten() {
            if let (Ok(iu), Ok(tu)) = (Uuid::parse_str(&item_uuid), Uuid::parse_str(&task_uuid)) {
                map.entry(iu).or_default().push(tu);
            }
        }
    }
    map
}

/// Usage-reinforcement window and weights: each time a memory surfaces in
/// recall within the window it earns a small boost. Five recalls in the
/// window lift a Weak (1.0) memory to Linked (1.5) — protecting it from
/// age-based pruning — but usage alone can never fabricate Strong (2.0).
pub const RECALL_BOOST_WINDOW_DAYS: i64 = 30;
pub const RECALL_BOOST_PER_HIT: f64 = 0.1;
pub const RECALL_BOOST_CAP: f64 = 0.5;

/// Log that a memory surfaced in recall output. Fire-and-forget semantics at
/// call sites — a failed write must never break a read path.
pub fn record_memory_recall(conn: &Connection, item_uuid: &Uuid) -> Result<()> {
    record_event(
        conn,
        "memory_recalled",
        Some(item_uuid),
        Some("memory"),
        &[],
        None,
    )
}

/// Record that a memory *surfaced* as an uninvited side-effect of spreading
/// activation, rather than being deliberately recalled. Logged under a distinct
/// action so it feeds Hebbian co-activation (memories that fire together still
/// link, see [`memory_recall_events_since`]) WITHOUT inflating strength the way
/// a deliberate recall does (see [`recall_usage_boost`]) — an associative hit
/// the agent never queried must not climb the ranking on graph centrality alone.
pub fn record_memory_surfaced(conn: &Connection, item_uuid: &Uuid) -> Result<()> {
    record_event(
        conn,
        "memory_surfaced",
        Some(item_uuid),
        Some("memory"),
        &[],
        None,
    )
}

/// Recall-derived reinforcement: how often this memory actually surfaced in
/// the last [`RECALL_BOOST_WINDOW_DAYS`] days, translated into a strength
/// boost. Events older than the window (or pruned by retention) don't count,
/// so unused memories decay back toward their base strength.
fn recall_usage_boost(conn: &Connection, item_uuid: &Uuid) -> f64 {
    let cutoff = dt_to_str(&(Utc::now() - chrono::Duration::days(RECALL_BOOST_WINDOW_DAYS)));
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events
             WHERE action='memory_recalled' AND ref_uuid=?1 AND at >= ?2",
            rusqlite::params![item_uuid.to_string(), cutoff],
            |r| r.get(0),
        )
        .unwrap_or(0);
    (count as f64 * RECALL_BOOST_PER_HIT).min(RECALL_BOOST_CAP)
}

/// Batched form of [`recall_usage_boost`]: the recall-usage boost for *every*
/// memory that surfaced in the window, computed in a single grouped query.
/// A consumer scoring many memories at once (e.g. building the memory graph)
/// runs one query here instead of one `COUNT(*)` per memory. Memories absent
/// from the map earned no recalls in the window (boost 0.0). The per-memory
/// value is identical to [`recall_usage_boost`].
pub fn recall_usage_boosts(conn: &Connection) -> std::collections::HashMap<Uuid, f64> {
    let cutoff = dt_to_str(&(Utc::now() - chrono::Duration::days(RECALL_BOOST_WINDOW_DAYS)));
    let mut map: std::collections::HashMap<Uuid, f64> = std::collections::HashMap::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT ref_uuid, COUNT(*) FROM events
         WHERE action='memory_recalled' AND ref_uuid IS NOT NULL AND at >= ?1
         GROUP BY ref_uuid",
    ) else {
        return map;
    };
    let rows = stmt.query_map([cutoff], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    });
    if let Ok(rows) = rows {
        for (uuid_str, count) in rows.flatten() {
            if let Ok(u) = Uuid::parse_str(&uuid_str) {
                map.insert(
                    u,
                    (count as f64 * RECALL_BOOST_PER_HIT).min(RECALL_BOOST_CAP),
                );
            }
        }
    }
    map
}

/// [`item_strength`] with a pre-computed recall-usage boost supplied by the
/// caller (see [`recall_usage_boosts`]). Behaviourally identical to
/// `item_strength` when `recall_boost` is that memory's boost; it just lets a
/// batch consumer avoid the per-item boost query.
pub fn item_strength_with_boost(conn: &Connection, item: &Item, recall_boost: f64) -> f64 {
    item_base_strength(conn, item) + recall_boost
}

/// Batched form of [`memory_recall_daily_counts`]: the same per-day buckets for
/// **every** memory that was recalled in the window, from a single query.
///
/// The per-item form scans the `memory_recalled` event log once per memory, so
/// a caller scoring the whole store (the `sara dream` constellation) paid
/// O(memories × recall events). The bucketing arithmetic here is deliberately
/// identical to the per-item version — including the `0..days` age filter that
/// drops both future timestamps and the exact-boundary day — so the two agree
/// exactly. Memories absent from the map had no recalls in the window.
/// Pinned by `memory_recall_daily_counts_batch_matches_per_item`.
pub fn memory_recall_daily_counts_all(
    conn: &Connection,
    days: i64,
) -> std::collections::HashMap<Uuid, Vec<u64>> {
    let mut out: std::collections::HashMap<Uuid, Vec<u64>> = std::collections::HashMap::new();
    let now = Utc::now();
    let cutoff = dt_to_str(&(now - chrono::Duration::days(days)));
    let Ok(mut stmt) = conn.prepare(
        "SELECT ref_uuid, at FROM events
         WHERE action='memory_recalled' AND ref_uuid IS NOT NULL AND at >= ?1",
    ) else {
        return out;
    };
    let rows = stmt.query_map([cutoff], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    });
    if let Ok(rows) = rows {
        for (uuid_str, at) in rows.flatten() {
            let Ok(u) = Uuid::parse_str(&uuid_str) else {
                continue;
            };
            if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&at) {
                let age_days = (now - ts.with_timezone(&Utc)).num_days();
                if (0..days).contains(&age_days) {
                    let idx = (days - 1 - age_days) as usize;
                    out.entry(u)
                        .or_insert_with(|| vec![0u64; days.max(1) as usize])[idx] += 1;
                }
            }
        }
    }
    out
}

/// Per-day recall counts for the last `days` days (oldest day first). Powers
/// the "recall pulse" sparkline in `sara dream` — a 30-day activity trace of
/// how often the memory actually surfaced.
pub fn memory_recall_daily_counts(conn: &Connection, item_uuid: &Uuid, days: i64) -> Vec<u64> {
    let mut counts = vec![0u64; days.max(1) as usize];
    let now = Utc::now();
    let cutoff = dt_to_str(&(now - chrono::Duration::days(days)));
    let Ok(mut stmt) = conn.prepare(
        "SELECT at FROM events
         WHERE action='memory_recalled' AND ref_uuid=?1 AND at >= ?2",
    ) else {
        return counts;
    };
    let rows = stmt
        .query_map(rusqlite::params![item_uuid.to_string(), cutoff], |r| {
            r.get::<_, String>(0)
        })
        .map(|rows| rows.flatten().collect::<Vec<_>>())
        .unwrap_or_default();
    for at in rows {
        if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&at) {
            let age_days = (now - ts.with_timezone(&Utc)).num_days();
            if (0..days).contains(&age_days) {
                let idx = (days - 1 - age_days) as usize;
                counts[idx] += 1;
            }
        }
    }
    counts
}
