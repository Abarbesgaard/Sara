use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::infrastructure::db;

pub fn doing_value(
    conn: &Connection,
    id: &str,
    text: &str,
    client: Option<&str>,
) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let entry = db::record_doing(conn, &task.uuid, text, client)?;
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "doing": entry.text,
        "at": entry.at.to_rfc3339(),
    }))
}

pub fn run(conn: &Connection, id: &str, text: &str) -> Result<()> {
    let v = doing_value(conn, id, text, Some("cli"))?;
    println!(
        "Task {} is now: {}",
        v["task"].as_i64().unwrap_or(0),
        v["doing"].as_str().unwrap_or_default()
    );
    Ok(())
}
