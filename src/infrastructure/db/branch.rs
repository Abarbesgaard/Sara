//! Per-task git branch snapshots.
//!
//! Split out of the db monolith (issue #168); re-exported by `super`.

use super::*;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use uuid::Uuid;

// ── branch snapshots ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BranchRecord {
    pub branch: String,
    pub base: Option<String>,
    /// Files changed on `branch` since merge-base with `base`; None until first snapshot.
    pub files: Option<Vec<String>>,
    pub logged_at: Option<DateTime<Utc>>,
}

fn parse_files_json(s: Option<String>) -> Option<Vec<String>> {
    s.and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok())
}

pub fn set_task_branch(conn: &Connection, task_uuid: &Uuid, branch: &str) -> Result<()> {
    // Get previous branch for history.
    let prev = get_task_branch(conn, task_uuid).map(|r| r.branch);
    conn.execute(
        "INSERT INTO task_branches (task_uuid, branch)
         VALUES (?1, ?2)
         ON CONFLICT(task_uuid) DO UPDATE SET
           branch             = ?2,
           base               = NULL,
           changed_files_json = NULL,
           logged_at          = NULL",
        params![task_uuid.to_string(), branch],
    )?;
    record_history(conn, task_uuid, "branch", prev.as_deref(), Some(branch))?;
    Ok(())
}

pub fn log_branch_changes(
    conn: &Connection,
    task_uuid: &Uuid,
    base: &str,
    files: &[String],
) -> Result<()> {
    let json = serde_json::to_string(files)?;
    let now = dt_to_str(&Utc::now());
    conn.execute(
        "UPDATE task_branches SET base=?2, changed_files_json=?3, logged_at=?4
         WHERE task_uuid=?1",
        params![task_uuid.to_string(), base, json, now],
    )?;
    Ok(())
}

pub fn get_task_branch(conn: &Connection, task_uuid: &Uuid) -> Option<BranchRecord> {
    conn.query_row(
        "SELECT branch, base, changed_files_json, logged_at FROM task_branches WHERE task_uuid=?1",
        [task_uuid.to_string()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        },
    )
    .ok()
    .map(|(branch, base, files_json, logged_at)| BranchRecord {
        branch,
        base,
        files: parse_files_json(files_json),
        logged_at: logged_at.and_then(|s| str_to_dt(&s).ok()),
    })
}

pub fn clear_task_branch(conn: &Connection, task_uuid: &Uuid) -> Result<()> {
    let prev = get_task_branch(conn, task_uuid).map(|r| r.branch);
    let n = conn.execute(
        "DELETE FROM task_branches WHERE task_uuid=?1",
        [task_uuid.to_string()],
    )?;
    if n > 0 {
        record_history(conn, task_uuid, "branch", prev.as_deref(), None)?;
    }
    Ok(())
}

/// All pending tasks in `project` (excluding `exclude_uuid`) that have a branch record.
/// Returns `(task_id, description, BranchRecord)`.
pub fn branched_pending_in_project(
    conn: &Connection,
    project: &str,
    exclude_uuid: &Uuid,
) -> Result<Vec<(i64, String, BranchRecord)>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.description, tb.branch, tb.base, tb.changed_files_json, tb.logged_at
         FROM tasks t
         JOIN task_branches tb ON tb.task_uuid = t.uuid
         WHERE t.project=?1 AND t.status='pending' AND t.uuid != ?2
         ORDER BY t.id ASC",
    )?;
    let rows = stmt.query_map(params![project, exclude_uuid.to_string()], |row| {
        Ok((
            row.get::<_, Option<i64>>(0)?.unwrap_or(0),
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    let mut result = vec![];
    for row in rows {
        let (id, desc, branch, base, files_json, logged_at) = row?;
        result.push((
            id,
            desc,
            BranchRecord {
                branch,
                base,
                files: parse_files_json(files_json),
                logged_at: logged_at.and_then(|s| str_to_dt(&s).ok()),
            },
        ));
    }
    Ok(result)
}
