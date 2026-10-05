use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use super::support::{kind_arg, one_based};
use crate::commands::shared::print_json;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

pub fn step_remove_value(
    conn: &Connection,
    id: &str,
    n: usize,
    kind: Option<&str>,
) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let kind = kind_arg(kind);
    let steps = db::get_steps(conn, &task.uuid, kind)?;
    let idx = one_based(n, kind)?;
    let item = steps
        .get(idx)
        .ok_or_else(|| anyhow::anyhow!("No {kind} #{n} on this task"))?;
    let text = item.text.clone();
    db::delete_step(conn, item.id)?;
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "kind": kind,
        "index": n,
        "removed": text,
    }))
}

pub fn step_remove_by_id_value(conn: &Connection, step_id: i64) -> Result<serde_json::Value> {
    let (uuid, kind, index) = db::locate_step(conn, step_id)?;
    step_remove_value(conn, &uuid.to_string(), index, Some(&kind))
}

pub fn step_remove(
    conn: &Connection,
    _cfg: &Config,
    id: &str,
    n: usize,
    kind: Option<&str>,
    as_json: bool,
) -> Result<()> {
    let v = step_remove_value(conn, id, n, kind)?;
    if as_json {
        print_json(&v)?;
        return Ok(());
    }
    println!(
        "Removed {} {} of task {}: {}",
        v["kind"].as_str().unwrap_or("step"),
        n,
        v["task"].as_i64().unwrap_or(0),
        v["removed"].as_str().unwrap_or_default()
    );
    Ok(())
}
