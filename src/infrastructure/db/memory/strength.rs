use super::super::*;
use crate::domain::services::memory_strength::{
    RECALL_BOOST_WINDOW_DAYS, base_strength, derived_bonus, recall_boost,
};
use crate::domain::{Item, Relation, Status};
use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

pub fn item_strength(conn: &Connection, item: &Item) -> f64 {
    item_base_strength(conn, item)
        + recall_usage_boost(conn, &item.uuid)
        + canonical_derived_bonus(conn, &item.uuid)
}

pub fn item_strengths(conn: &Connection, items: &[Item]) -> std::collections::HashMap<Uuid, f64> {
    let bases = item_base_strengths(conn, items);
    let boosts = recall_usage_boosts(conn);
    let bonuses = canonical_derived_bonuses(conn);
    items
        .iter()
        .map(|item| {
            let s = bases.get(&item.uuid).copied().unwrap_or(1.0)
                + boosts.get(&item.uuid).copied().unwrap_or(0.0)
                + bonuses.get(&item.uuid).copied().unwrap_or(0.0);
            (item.uuid, s)
        })
        .collect()
}

fn canonical_derived_bonus(conn: &Connection, item_uuid: &Uuid) -> f64 {
    let count = get_memory_links_to(conn, &item_uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == Relation::DerivedFrom)
        .count();
    derived_bonus(count)
}

fn item_base_strength(conn: &Connection, item: &Item) -> f64 {
    let source = item
        .source_task_uuid
        .and_then(|source| {
            get_task_by_uuid_prefix(conn, &source.to_string())
                .ok()
                .flatten()
        })
        .map(|task| task.status);
    let linked: Vec<Status> = if source.is_some() {
        Vec::new()
    } else {
        get_item_task_links(conn, &item.uuid)
            .unwrap_or_default()
            .into_iter()
            .map(|(t, _)| t.status)
            .collect()
    };
    base_strength(item.status, source.as_ref(), &linked)
}

pub fn item_base_strengths(
    conn: &Connection,
    items: &[Item],
) -> std::collections::HashMap<Uuid, f64> {
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
        let source = item
            .source_task_uuid
            .and_then(|s| task_status.get(&s.to_string()));
        let linked = link_status.get(&item.uuid).map_or(&[][..], |v| v);
        out.insert(item.uuid, base_strength(item.status, source, linked));
    }
    out
}

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
        .map(|(u, c)| (u, derived_bonus(c as usize)))
        .collect()
}

fn status_from_str(s: &str) -> Status {
    match s {
        "completed" => Status::Completed,
        "deleted" => Status::Deleted,
        _ => Status::Pending,
    }
}

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

pub fn record_memory_recall(conn: &Connection, item_uuid: &Uuid) -> Result<()> {
    super::uses::attribute_use(conn, item_uuid, crate::domain::MemoryUseKind::Recalled);
    record_event(
        conn,
        "memory_recalled",
        Some(item_uuid),
        Some("memory"),
        &[],
        None,
    )
}

pub fn record_memory_surfaced(conn: &Connection, item_uuid: &Uuid) -> Result<()> {
    super::uses::attribute_use(conn, item_uuid, crate::domain::MemoryUseKind::Surfaced);
    record_event(
        conn,
        "memory_surfaced",
        Some(item_uuid),
        Some("memory"),
        &[],
        None,
    )
}

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
    recall_boost(count)
}

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
                map.insert(u, recall_boost(count));
            }
        }
    }
    map
}

pub fn item_strength_with_boost(conn: &Connection, item: &Item, recall_boost: f64) -> f64 {
    item_base_strength(conn, item) + recall_boost
}

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
