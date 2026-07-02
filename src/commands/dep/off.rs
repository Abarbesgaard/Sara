use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;

/// Remove a dependency. Print-free core shared by the CLI `dep off` and MCP `dep`.
pub fn dep_off_value(conn: &Connection, cfg: &Config, id: &str, other: &str) -> Result<Value> {
    let task = db::resolve_task(conn, id)?;
    let dep = db::resolve_task(conn, other)?;

    db::remove_dependency(conn, &task.uuid, &dep.uuid)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;
    db::refresh_urgency(conn, &cfg.urgency, &dep.uuid)?;

    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "action": "off",
        "removed": { "id": dep.id, "uuid": dep.uuid.to_string(), "description": dep.description },
    }))
}

pub fn run_off(conn: &Connection, cfg: &Config, id: &str, other: &str) -> Result<()> {
    let v = dep_off_value(conn, cfg, id, other)?;
    println!(
        "Removed dependency: task {} no longer depends on task {}",
        v["task"].as_i64().unwrap_or(0),
        v["removed"]["id"].as_i64().unwrap_or(0),
    );
    Ok(())
}
