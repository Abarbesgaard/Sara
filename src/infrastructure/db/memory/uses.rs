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
