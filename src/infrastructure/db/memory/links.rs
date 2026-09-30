use super::super::*;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct MemoryLink {
    pub id: i64,
    pub from_uuid: String,
    pub to_uuid: String,
    pub relation: String,
    pub weight: f64,
}

pub const MEMORY_LINK_RELATIONS: &[&str] = &[
    "supersedes",
    "similar_to",
    "derived_from",
    "used_in",
    "co_activated",
];

pub fn insert_memory_link(
    conn: &Connection,
    from_uuid: &str,
    to_uuid: &str,
    relation: &str,
    weight: f64,
) -> Result<()> {
    if !MEMORY_LINK_RELATIONS.contains(&relation) {
        anyhow::bail!(
            "Unknown relation '{}'. Valid types: {}",
            relation,
            MEMORY_LINK_RELATIONS.join(", ")
        );
    }
    if from_uuid == to_uuid {
        anyhow::bail!("A memory cannot link to itself.");
    }
    if matches!(relation, "supersedes" | "derived_from")
        && memory_link_path_exists(conn, to_uuid, from_uuid)?
    {
        anyhow::bail!(
            "Inserting this link would create a cycle \
                 (a path from '{}' to '{}' already exists).",
            to_uuid,
            from_uuid
        );
    }
    conn.execute(
        "INSERT OR IGNORE INTO memory_links (from_uuid, to_uuid, relation, weight, created)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![from_uuid, to_uuid, relation, weight, dt_to_str(&Utc::now())],
    )?;
    Ok(())
}

fn memory_link_path_exists(conn: &Connection, start: &str, target: &str) -> Result<bool> {
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    queue.push_back(start.to_string());
    while let Some(current) = queue.pop_front() {
        if current == target {
            return Ok(true);
        }
        if !visited.insert(current.clone()) {
            continue;
        }
        let mut stmt = conn.prepare(
            "SELECT to_uuid FROM memory_links
             WHERE from_uuid = ?1 AND relation IN ('supersedes', 'derived_from')",
        )?;
        let neighbours: Vec<String> = stmt
            .query_map([&current], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        for n in neighbours {
            if !visited.contains(&n) {
                queue.push_back(n);
            }
        }
    }
    Ok(false)
}

pub fn all_memory_links(conn: &Connection) -> Result<Vec<MemoryLink>> {
    let mut stmt = conn.prepare(
        "SELECT id, from_uuid, to_uuid, relation, weight FROM memory_links ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(MemoryLink {
                id: r.get(0)?,
                from_uuid: r.get(1)?,
                to_uuid: r.get(2)?,
                relation: r.get(3)?,
                weight: r.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Links with an endpoint that no longer resolves to a live memory: missing from
/// `items`, or archived — except the archived *target* of a `supersedes` edge,
/// which is the expected end state of superseding.
pub fn orphaned_memory_links(conn: &Connection) -> Result<Vec<MemoryLink>> {
    let mut stmt = conn.prepare(
        "SELECT ml.id, ml.from_uuid, ml.to_uuid, ml.relation, ml.weight
         FROM memory_links ml
         LEFT JOIN items f ON f.uuid = ml.from_uuid
         LEFT JOIN items t ON t.uuid = ml.to_uuid
         WHERE f.uuid IS NULL OR t.uuid IS NULL
            OR f.status NOT IN ('active','provisional')
            OR (t.status NOT IN ('active','provisional') AND ml.relation <> 'supersedes')
         ORDER BY ml.id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(MemoryLink {
            id: r.get(0)?,
            from_uuid: r.get(1)?,
            to_uuid: r.get(2)?,
            relation: r.get(3)?,
            weight: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get_memory_links_from(conn: &Connection, from_uuid: &str) -> Result<Vec<MemoryLink>> {
    let mut stmt = conn.prepare(
        "SELECT id, from_uuid, to_uuid, relation, weight
         FROM memory_links WHERE from_uuid = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map([from_uuid], |r| {
            Ok(MemoryLink {
                id: r.get(0)?,
                from_uuid: r.get(1)?,
                to_uuid: r.get(2)?,
                relation: r.get(3)?,
                weight: r.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn get_memory_links_to(conn: &Connection, to_uuid: &str) -> Result<Vec<MemoryLink>> {
    let mut stmt = conn.prepare(
        "SELECT id, from_uuid, to_uuid, relation, weight
         FROM memory_links WHERE to_uuid = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map([to_uuid], |r| {
            Ok(MemoryLink {
                id: r.get(0)?,
                from_uuid: r.get(1)?,
                to_uuid: r.get(2)?,
                relation: r.get(3)?,
                weight: r.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn delete_memory_link(
    conn: &Connection,
    from_uuid: &str,
    to_uuid: &str,
    relation: &str,
) -> Result<bool> {
    let changed = conn.execute(
        "DELETE FROM memory_links WHERE from_uuid=?1 AND to_uuid=?2 AND relation=?3",
        rusqlite::params![from_uuid, to_uuid, relation],
    )?;
    Ok(changed > 0)
}

pub fn replace_coactivations(conn: &Connection, edges: &[(String, String, f64)]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM memory_links WHERE relation = 'co_activated'",
        [],
    )?;
    {
        let now = dt_to_str(&Utc::now());
        let mut stmt = tx.prepare(
            "INSERT INTO memory_links (from_uuid, to_uuid, relation, weight, created)
             VALUES (?1, ?2, 'co_activated', ?3, ?4)
             ON CONFLICT(from_uuid, to_uuid, relation)
             DO UPDATE SET weight = weight + excluded.weight",
        )?;
        for (a, b, w) in edges {
            if a == b {
                continue;
            }
            let (from, to) = if a < b { (a, b) } else { (b, a) };
            stmt.execute(rusqlite::params![from, to, w, now])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn memory_recall_events_since(
    conn: &Connection,
    cutoff: &DateTime<Utc>,
) -> Result<Vec<(Uuid, DateTime<Utc>)>> {
    let mut stmt = conn.prepare(
        "SELECT ref_uuid, at FROM events
         WHERE action IN ('memory_recalled','memory_surfaced') AND ref_uuid IS NOT NULL AND at >= ?1
         ORDER BY at ASC",
    )?;
    let rows = stmt.query_map([dt_to_str(cutoff)], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = vec![];
    for r in rows {
        let (u, at) = r?;
        if let (Ok(uuid), Ok(dt)) = (Uuid::parse_str(&u), str_to_dt(&at)) {
            out.push((uuid, dt));
        }
    }
    Ok(out)
}
