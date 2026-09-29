use super::*;
use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

pub struct ChecklistItem {
    pub id: i64,
    pub text: String,
    pub done: bool,
    pub position: i64,
    pub intent: Option<String>,
    pub kind: String,
    pub source: String,
    pub verify_cmd: Option<String>,
    pub result: Option<String>,
    pub done_commit: Option<String>,
    pub done_at: Option<String>,
}

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

pub fn add_step(
    conn: &Connection,
    task_uuid: &Uuid,
    text: &str,
    intent: Option<&str>,
    kind: &str,
    source: &str,
    verify_cmd: Option<&str>,
) -> Result<i64> {
    with_write_lock(conn, || {
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
