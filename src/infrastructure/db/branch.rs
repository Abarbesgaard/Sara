use super::*;
use anyhow::Result;
use rusqlite::{Connection, params};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct BranchRecord {
    pub branch: String,
}

pub fn set_task_branch(conn: &Connection, task_uuid: &Uuid, branch: &str) -> Result<()> {
    let prev = get_task_branch(conn, task_uuid).map(|r| r.branch);
    conn.execute(
        "INSERT INTO task_branches (task_uuid, branch)
         VALUES (?1, ?2)
         ON CONFLICT(task_uuid) DO UPDATE SET branch = ?2",
        params![task_uuid.to_string(), branch],
    )?;
    record_history(conn, task_uuid, "branch", prev.as_deref(), Some(branch))?;
    Ok(())
}

pub fn get_task_branch(conn: &Connection, task_uuid: &Uuid) -> Option<BranchRecord> {
    conn.query_row(
        "SELECT branch FROM task_branches WHERE task_uuid=?1",
        [task_uuid.to_string()],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .map(|branch| BranchRecord { branch })
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
