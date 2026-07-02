use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;

/// Add a dependency (task `id` becomes blocked by `other`), returning a structured
/// record. Print-free core shared by the CLI `dep on` and the MCP `dep` tool.
pub fn dep_on_value(conn: &Connection, cfg: &Config, id: &str, other: &str) -> Result<Value> {
    let task = db::resolve_task(conn, id)?;
    let dep = db::resolve_task(conn, other)?;

    db::add_dependency(conn, &task.uuid, &dep.uuid)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;
    db::refresh_urgency(conn, &cfg.urgency, &dep.uuid)?;

    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "action": "on",
        "depends_on": { "id": dep.id, "uuid": dep.uuid.to_string(), "description": dep.description },
    }))
}

pub fn run_on(conn: &Connection, cfg: &Config, id: &str, other: &str) -> Result<()> {
    let v = dep_on_value(conn, cfg, id, other)?;
    println!(
        "Task {} now depends on task {} (\"{}\")",
        v["task"].as_i64().unwrap_or(0),
        v["depends_on"]["id"].as_i64().unwrap_or(0),
        v["depends_on"]["description"].as_str().unwrap_or_default()
    );
    Ok(())
}
