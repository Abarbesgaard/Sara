//! Task checklist / steps / acceptance criteria.
//!
//! Split out of the db monolith (issue #168); re-exported by `super`.

use super::*;
use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

// ── checklist ─────────────────────────────────────────────────────────────────

pub struct ChecklistItem {
    pub id: i64,
    pub text: String,
    pub done: bool,
    pub position: i64,
    /// Fuller "what this step does" written by the LLM (None for legacy items).
    pub intent: Option<String>,
    /// "step" (default) or "acceptance" (a definition-of-done criterion).
    pub kind: String,
    /// "human" or "ai".
    pub source: String,
    /// Command that verifies this step / criterion.
    pub verify_cmd: Option<String>,
    /// Execution outcome recorded when the step is marked done.
    pub result: Option<String>,
    /// Git commit the step was completed at.
    pub done_commit: Option<String>,
    pub done_at: Option<String>,
}

/// Step kinds.
pub const STEP_KIND_STEP: &str = "step";
pub const STEP_KIND_ACCEPTANCE: &str = "acceptance";

fn row_to_step(r: &rusqlite::Row<'_>) -> rusqlite::Result<ChecklistItem> {
    Ok(ChecklistItem {
        id: r.get(0)?,
        text: r.get(1)?,
        done: r.get::<_, i64>(2)? != 0,
        position: r.get(3)?,
        intent: r.get(4)?,
        kind: r.get(5)?,
        source: r.get(6)?,
        verify_cmd: r.get(7)?,
        result: r.get(8)?,
        done_commit: r.get(9)?,
        done_at: r.get(10)?,
    })
}

const STEP_COLUMNS: &str =
    "id, text, done, position, intent, kind, source, verify_cmd, result, done_commit, done_at";

pub fn get_checklist(conn: &Connection, task_uuid: &Uuid) -> Result<Vec<ChecklistItem>> {
    let sql = format!(
        "SELECT {STEP_COLUMNS} FROM task_checklist WHERE task_uuid=?1 ORDER BY position, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let items = stmt
        .query_map([task_uuid.to_string()], row_to_step)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(items)
}

/// Steps (or acceptance criteria) for a task, filtered by kind, ordered.
pub fn get_steps(conn: &Connection, task_uuid: &Uuid, kind: &str) -> Result<Vec<ChecklistItem>> {
    let sql = format!(
        "SELECT {STEP_COLUMNS} FROM task_checklist WHERE task_uuid=?1 AND kind=?2 ORDER BY position, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let items = stmt
        .query_map(rusqlite::params![task_uuid.to_string(), kind], row_to_step)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(items)
}

pub fn add_checklist_item(conn: &Connection, task_uuid: &Uuid, text: &str) -> Result<()> {
    add_step(conn, task_uuid, text, None, STEP_KIND_STEP, "human", None).map(|_| ())
}

/// Insert a step / acceptance criterion with full metadata; returns its id.
pub fn add_step(
    conn: &Connection,
    task_uuid: &Uuid,
    text: &str,
    intent: Option<&str>,
    kind: &str,
    source: &str,
    verify_cmd: Option<&str>,
) -> Result<i64> {
    // Position allocation is read-then-write, so it needs the write lock for
    // the same reason `next_display_id` does — two concurrent `check` calls
    // would otherwise both claim the same position.
    with_write_lock(conn, || {
        // A DB error here must propagate: COALESCE guarantees a row, so the only
        // way this fails is a real error (SQLITE_BUSY, I/O). Falling back to
        // position 1 would silently insert the step at the *front*, duplicating an
        // existing position and scrambling the guide's step order.
        let pos: i64 = conn.query_row(
            "SELECT COALESCE(MAX(position),0)+1 FROM task_checklist WHERE task_uuid=?1",
            [task_uuid.to_string()],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT INTO task_checklist (task_uuid, text, done, position, intent, kind, source, verify_cmd)
             VALUES (?1,?2,0,?3,?4,?5,?6,?7)",
            rusqlite::params![task_uuid.to_string(), text, pos, intent, kind, source, verify_cmd],
        )?;
        let id = conn.last_insert_rowid();
        let label = if kind == STEP_KIND_ACCEPTANCE {
            "acceptance"
        } else {
            "checklist"
        };
        record_history(conn, task_uuid, label, None, Some(text))?;
        Ok(id)
    })
}

/// Mark a step done/undone, recording the execution result and git commit.
pub fn set_step_done(
    conn: &Connection,
    item_id: i64,
    done: bool,
    result: Option<&str>,
    commit: Option<&str>,
) -> Result<()> {
    let task_uuid_str: String = conn.query_row(
        "SELECT task_uuid FROM task_checklist WHERE id=?1",
        [item_id],
        |r| r.get(0),
    )?;
    if done {
        conn.execute(
            "UPDATE task_checklist SET done=1, result=COALESCE(?2,result), done_commit=?3, done_at=?4 WHERE id=?1",
            rusqlite::params![item_id, result, commit, dt_to_str(&Utc::now())],
        )?;
    } else {
        conn.execute(
            "UPDATE task_checklist SET done=0, done_commit=NULL, done_at=NULL WHERE id=?1",
            [item_id],
        )?;
    }
    if let Ok(uuid) = Uuid::parse_str(&task_uuid_str) {
        record_history(
            conn,
            &uuid,
            "checklist",
            None,
            Some(if done { "step done" } else { "step reopened" }),
        )?;
    }
    Ok(())
}

/// Find a step id by its 1-based position among the task's steps of a kind.
pub fn step_id_by_index(
    conn: &Connection,
    task_uuid: &Uuid,
    kind: &str,
    index: usize,
) -> Result<i64> {
    let steps = get_steps(conn, task_uuid, kind)?;
    steps
        .get(index.saturating_sub(1))
        .map(|s| s.id)
        .ok_or_else(|| anyhow::anyhow!("No {kind} #{index} on this task"))
}

/// Reverse of `step_id_by_index`: given a checklist row id, return the task it
/// belongs to, its kind, and its 1-based position within that kind. Lets MCP
/// callers address an item by the `step_id` that `check` handed back, instead of
/// having to know its position.
pub fn locate_step(conn: &Connection, step_id: i64) -> Result<(Uuid, String, usize)> {
    let (task_uuid_str, kind): (String, String) = conn
        .query_row(
            "SELECT task_uuid, kind FROM task_checklist WHERE id=?1",
            [step_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| anyhow::anyhow!("No checklist item with step_id {step_id}"))?;
    let uuid = Uuid::parse_str(&task_uuid_str)?;
    let steps = get_steps(conn, &uuid, &kind)?;
    let index = steps
        .iter()
        .position(|s| s.id == step_id)
        .map(|i| i + 1)
        .ok_or_else(|| anyhow::anyhow!("step_id {step_id} not found within its kind"))?;
    Ok((uuid, kind, index))
}

/// Move a checklist item one slot up (`up=true`) or down within its own kind
/// (steps reorder among steps, acceptance among acceptance). Returns `true` if
/// the order changed, `false` when the item is already at the section boundary.
///
/// The set of `position` slots occupied by the kind is preserved and the rows
/// are reassigned to them in the new order, so items of the other kind are never
/// disturbed or interleaved.
pub fn move_step(conn: &Connection, item_id: i64, up: bool) -> Result<bool> {
    let (task_uuid_str, kind): (String, String) = conn.query_row(
        "SELECT task_uuid, kind FROM task_checklist WHERE id=?1",
        [item_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let uuid = Uuid::parse_str(&task_uuid_str)?;

    let mut items = get_steps(conn, &uuid, &kind)?;
    let Some(idx) = items.iter().position(|s| s.id == item_id) else {
        return Ok(false);
    };
    let target = if up {
        if idx == 0 {
            return Ok(false);
        }
        idx - 1
    } else {
        if idx + 1 >= items.len() {
            return Ok(false);
        }
        idx + 1
    };

    // Reassign the kind's rows to their existing position slots in the new order.
    let slots: Vec<i64> = items.iter().map(|s| s.position).collect();
    items.swap(idx, target);
    for (slot, it) in slots.iter().zip(items.iter()) {
        conn.execute(
            "UPDATE task_checklist SET position=?1 WHERE id=?2",
            rusqlite::params![slot, it.id],
        )?;
    }
    record_history(conn, &uuid, "checklist", None, Some("step reordered"))?;
    Ok(true)
}

/// Remove a step / acceptance criterion by its stable row id, recording history.
/// Remaining items keep their `position`, so the 1-based display order stays
/// consistent (later items simply shift up by one).
pub fn delete_step(conn: &Connection, item_id: i64) -> Result<()> {
    let (task_uuid_str, text, kind): (String, String, String) = conn.query_row(
        "SELECT task_uuid, text, kind FROM task_checklist WHERE id=?1",
        [item_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    conn.execute("DELETE FROM task_checklist WHERE id=?1", [item_id])?;
    if let Ok(uuid) = Uuid::parse_str(&task_uuid_str) {
        let label = if kind == STEP_KIND_ACCEPTANCE {
            "acceptance"
        } else {
            "checklist"
        };
        record_history(conn, &uuid, label, Some(&text), Some("removed"))?;
    }
    Ok(())
}

pub fn toggle_checklist_item(conn: &Connection, item_id: i64) -> Result<bool> {
    let (task_uuid_str, text, done): (String, String, i64) = conn.query_row(
        "SELECT task_uuid, text, done FROM task_checklist WHERE id=?1",
        [item_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let new_done = if done == 0 { 1i64 } else { 0i64 };
    conn.execute(
        "UPDATE task_checklist SET done=?1 WHERE id=?2",
        rusqlite::params![new_done, item_id],
    )?;
    if let Ok(uuid) = Uuid::parse_str(&task_uuid_str) {
        let old = format!("{} {text}", if done == 0 { "[ ]" } else { "[x]" });
        let new = format!("{} {text}", if new_done == 0 { "[ ]" } else { "[x]" });
        record_history(conn, &uuid, "checklist", Some(&old), Some(&new))?;
    }
    Ok(new_done != 0)
}

pub fn delete_checklist_item(conn: &Connection, item_id: i64) -> Result<()> {
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT task_uuid, text FROM task_checklist WHERE id=?1",
            [item_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    let n = conn.execute("DELETE FROM task_checklist WHERE id=?1", [item_id])?;
    if n > 0
        && let Some((uuid_str, text)) = existing
        && let Ok(uuid) = Uuid::parse_str(&uuid_str)
    {
        record_history(conn, &uuid, "checklist", Some(&text), None)?;
    }
    Ok(())
}
