use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::infrastructure::db;

pub fn assignment_value(conn: &Connection, id: &str, text: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    db::set_assignment(conn, &task.uuid, text)?;
    Ok(json!({ "task": task.id, "uuid": task.uuid.to_string(), "assignment": text }))
}

pub fn assignment(conn: &Connection, id: &str, text: &str) -> Result<()> {
    let v = assignment_value(conn, id, text)?;
    println!(
        "Set assignment for task {}.",
        v["task"].as_i64().unwrap_or(0)
    );
    Ok(())
}

pub fn rationale_value(conn: &Connection, id: &str, text: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    db::set_rationale(conn, &task.uuid, text)?;
    Ok(json!({ "task": task.id, "uuid": task.uuid.to_string(), "rationale": text }))
}

pub fn rationale(conn: &Connection, id: &str, text: &str) -> Result<()> {
    let v = rationale_value(conn, id, text)?;
    println!(
        "Set rationale for task {}.",
        v["task"].as_i64().unwrap_or(0)
    );
    Ok(())
}
