use super::super::*;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use std::collections::HashMap;
use uuid::Uuid;

/// How a memory took part in a task. Outcome credit is derived from these at
/// read time, joined against the task's status and validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryUseKind {
    Surfaced,
    Recalled,
    Cited,
}

impl MemoryUseKind {
    pub const ALL: [MemoryUseKind; 3] = [Self::Surfaced, Self::Recalled, Self::Cited];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Surfaced => "surfaced",
            Self::Recalled => "recalled",
            Self::Cited => "cited",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryUse {
    pub item_uuid: Uuid,
    pub task_uuid: Uuid,
    pub kind: MemoryUseKind,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryUseCounts {
    pub surfaced: u64,
    pub recalled: u64,
    pub cited: u64,
}

impl MemoryUseCounts {
    fn add(&mut self, kind: MemoryUseKind, n: u64) {
        match kind {
            MemoryUseKind::Surfaced => self.surfaced += n,
            MemoryUseKind::Recalled => self.recalled += n,
            MemoryUseKind::Cited => self.cited += n,
        }
    }
}

thread_local! {
    static USE_TASK: std::cell::Cell<Option<Uuid>> = const { std::cell::Cell::new(None) };
}

struct UseTaskGuard(Option<Uuid>);

impl Drop for UseTaskGuard {
    fn drop(&mut self) {
        USE_TASK.with(|c| c.set(self.0));
    }
}

/// Runs `f` with every memory recall/surfacing inside it also recorded as a
/// use against `task`, so an explicit `recall --task` credits that task.
pub fn with_use_attribution<T>(task: Option<Uuid>, f: impl FnOnce() -> T) -> T {
    let _guard = UseTaskGuard(USE_TASK.with(|c| c.replace(task)));
    f()
}

pub(crate) fn attribute_use(conn: &Connection, item_uuid: &Uuid, kind: MemoryUseKind) {
    if let Some(task) = USE_TASK.with(|c| c.get()) {
        let _ = record_memory_use(conn, item_uuid, &task, kind);
    }
}

/// Idempotent: a repeat of the same (memory, task, kind) keeps the first `at`.
pub fn record_memory_use(
    conn: &Connection,
    item_uuid: &Uuid,
    task_uuid: &Uuid,
    kind: MemoryUseKind,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO memory_uses (item_uuid, task_uuid, kind, at)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            item_uuid.to_string(),
            task_uuid.to_string(),
            kind.as_str(),
            dt_to_str(&Utc::now())
        ],
    )?;
    Ok(())
}

fn row_to_use(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String, String, String)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

fn collect_uses(stmt: &mut rusqlite::Statement<'_>, param: &str) -> Result<Vec<MemoryUse>> {
    let rows = stmt.query_map([param], row_to_use)?;
    let mut out = Vec::new();
    for row in rows {
        let (item, task, kind, at) = row?;
        let Some(kind) = MemoryUseKind::parse(&kind) else {
            continue;
        };
        out.push(MemoryUse {
            item_uuid: Uuid::parse_str(&item)?,
            task_uuid: Uuid::parse_str(&task)?,
            kind,
            at: str_to_dt(&at)?,
        });
    }
    Ok(out)
}

pub fn memory_uses_for_task(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<MemoryUse>> {
    let mut stmt = conn.prepare(
        "SELECT item_uuid, task_uuid, kind, at FROM memory_uses
         WHERE task_uuid = ?1 ORDER BY at, item_uuid, kind",
    )?;
    collect_uses(&mut stmt, &task_uuid.to_string())
}

pub fn memory_uses_for_item(conn: &Connection, item_uuid: &Uuid) -> Result<Vec<MemoryUse>> {
    let mut stmt = conn.prepare(
        "SELECT item_uuid, task_uuid, kind, at FROM memory_uses
         WHERE item_uuid = ?1 ORDER BY at, task_uuid, kind",
    )?;
    collect_uses(&mut stmt, &item_uuid.to_string())
}

/// Per-item twin of [`memory_use_counts_all`]; the two must agree.
pub fn memory_use_counts(conn: &Connection, item_uuid: &Uuid) -> MemoryUseCounts {
    let mut counts = MemoryUseCounts::default();
    let Ok(mut stmt) =
        conn.prepare("SELECT kind, COUNT(*) FROM memory_uses WHERE item_uuid = ?1 GROUP BY kind")
    else {
        return counts;
    };
    if let Ok(rows) = stmt.query_map([item_uuid.to_string()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    }) {
        for (kind, n) in rows.flatten() {
            if let Some(kind) = MemoryUseKind::parse(&kind) {
                counts.add(kind, n as u64);
            }
        }
    }
    counts
}

/// Batched twin of [`memory_use_counts`]: one query for every memory with uses.
pub fn memory_use_counts_all(conn: &Connection) -> HashMap<Uuid, MemoryUseCounts> {
    let mut map: HashMap<Uuid, MemoryUseCounts> = HashMap::new();
    let Ok(mut stmt) =
        conn.prepare("SELECT item_uuid, kind, COUNT(*) FROM memory_uses GROUP BY item_uuid, kind")
    else {
        return map;
    };
    if let Ok(rows) = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    }) {
        for (item, kind, n) in rows.flatten() {
            if let (Ok(u), Some(kind)) = (Uuid::parse_str(&item), MemoryUseKind::parse(&kind)) {
                map.entry(u).or_default().add(kind, n as u64);
            }
        }
    }
    map
}

/// A memory's track record across tasks, counted in distinct non-deleted
/// tasks. `ignored` counts completed tasks that recalled or surfaced the
/// memory but never cited it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct MemoryProvenance {
    pub cited_verified: u64,
    pub cited: u64,
    pub recalled_in_tasks: u64,
    pub surfaced_in_tasks: u64,
    pub ignored: u64,
}

impl MemoryProvenance {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// e.g. `✓ cited in 3 verified tasks · cited in 1 · surfaced in 7`.
    pub fn summary(&self) -> String {
        let tasks = |n: u64| if n == 1 { "task" } else { "tasks" };
        let mut parts = Vec::new();
        if self.cited_verified > 0 {
            parts.push(format!(
                "✓ cited in {} verified {}",
                self.cited_verified,
                tasks(self.cited_verified)
            ));
        }
        let unverified = self.cited - self.cited_verified;
        if unverified > 0 {
            parts.push(format!("cited in {unverified} {}", tasks(unverified)));
        }
        if self.recalled_in_tasks > 0 {
            parts.push(format!("recalled in {}", self.recalled_in_tasks));
        }
        if self.surfaced_in_tasks > 0 {
            parts.push(format!("surfaced in {}", self.surfaced_in_tasks));
        }
        if self.ignored > 0 {
            parts.push(format!("ignored in {}", self.ignored));
        }
        parts.join(" · ")
    }
}

const PROVENANCE_SQL: &str = "
    SELECT u.item_uuid,
      COUNT(DISTINCT CASE WHEN u.kind = 'cited'
            AND t.status = 'completed' AND t.validated_commit IS NOT NULL
            THEN u.task_uuid END),
      COUNT(DISTINCT CASE WHEN u.kind = 'cited' THEN u.task_uuid END),
      COUNT(DISTINCT CASE WHEN u.kind = 'recalled' THEN u.task_uuid END),
      COUNT(DISTINCT CASE WHEN u.kind = 'surfaced' THEN u.task_uuid END),
      COUNT(DISTINCT CASE WHEN u.kind IN ('recalled', 'surfaced')
            AND t.status = 'completed'
            AND NOT EXISTS (SELECT 1 FROM memory_uses c
                            WHERE c.item_uuid = u.item_uuid AND c.task_uuid = u.task_uuid
                              AND c.kind = 'cited')
            THEN u.task_uuid END)
    FROM memory_uses u JOIN tasks t ON t.uuid = u.task_uuid
    WHERE t.status != 'deleted' AND (?1 IS NULL OR u.item_uuid = ?1)
    GROUP BY u.item_uuid";

fn provenance_rows(conn: &Connection, item: Option<String>) -> HashMap<Uuid, MemoryProvenance> {
    let mut out = HashMap::new();
    let Ok(mut stmt) = conn.prepare(PROVENANCE_SQL) else {
        return out;
    };
    let rows = stmt.query_map([item], |r| {
        Ok((
            r.get::<_, String>(0)?,
            MemoryProvenance {
                cited_verified: r.get::<_, i64>(1)? as u64,
                cited: r.get::<_, i64>(2)? as u64,
                recalled_in_tasks: r.get::<_, i64>(3)? as u64,
                surfaced_in_tasks: r.get::<_, i64>(4)? as u64,
                ignored: r.get::<_, i64>(5)? as u64,
            },
        ))
    });
    if let Ok(rows) = rows {
        for (uuid, p) in rows.flatten() {
            if let Ok(uuid) = Uuid::parse_str(&uuid) {
                out.insert(uuid, p);
            }
        }
    }
    out
}

pub fn memory_provenance(conn: &Connection, item_uuid: &Uuid) -> MemoryProvenance {
    provenance_rows(conn, Some(item_uuid.to_string()))
        .remove(item_uuid)
        .unwrap_or_default()
}

/// Batched twin of [`memory_provenance`]: one query for every memory with uses.
pub fn memory_provenance_all(conn: &Connection) -> HashMap<Uuid, MemoryProvenance> {
    provenance_rows(conn, None)
}

/// How many verified tasks drew on prior knowledge: the vision's KPI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReuseStats {
    /// Completed tasks with a validated commit, ended since the cutoff.
    pub verified: u64,
    /// ...of which cited or recalled a memory created before the task began.
    pub used_prior: u64,
    /// ...of which cited such a memory (the stricter measure).
    pub cited_prior: u64,
}

/// Counts verified tasks ended at or after `since`, optionally in one project.
pub fn knowledge_reuse(
    conn: &Connection,
    project: Option<&str>,
    since: &DateTime<Utc>,
) -> Result<ReuseStats> {
    let prior = |kinds: &str| {
        format!(
            "EXISTS (SELECT 1 FROM memory_uses u JOIN items i ON i.uuid = u.item_uuid
                     WHERE u.task_uuid = t.uuid AND {})",
            prior_use(kinds)
        )
    };
    let sql = format!(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN {} THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN {} THEN 1 ELSE 0 END), 0)
         FROM tasks t
         WHERE t.status = 'completed' AND t.validated_commit IS NOT NULL
           AND t.end >= ?1 AND (?2 IS NULL OR t.project = ?2)",
        prior("'cited', 'recalled'"),
        prior("'cited'"),
    );
    Ok(
        conn.query_row(&sql, rusqlite::params![since.to_rfc3339(), project], |r| {
            Ok(ReuseStats {
                verified: r.get::<_, i64>(0)? as u64,
                used_prior: r.get::<_, i64>(1)? as u64,
                cited_prior: r.get::<_, i64>(2)? as u64,
            })
        })?,
    )
}

/// A use (alias `u`, memory `i`, task `t`) of a memory that existed before the
/// task began: the "prior knowledge" both the KPI and telemetry count.
fn prior_use(kinds: &str) -> String {
    format!("u.kind IN ({kinds}) AND i.created < t.entry")
}

/// How many distinct memories created before the task began it cited or recalled.
pub fn prior_knowledge_used(conn: &Connection, task_uuid: &Uuid) -> Result<u64> {
    let sql = format!(
        "SELECT COUNT(DISTINCT u.item_uuid)
         FROM memory_uses u JOIN items i ON i.uuid = u.item_uuid
                            JOIN tasks t ON t.uuid = u.task_uuid
         WHERE t.uuid = ?1 AND {}",
        prior_use("'cited', 'recalled'")
    );
    Ok(conn.query_row(&sql, [task_uuid.to_string()], |r| r.get::<_, i64>(0))? as u64)
}
