use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::infrastructure::db;

/// Structured blockers/blocking view. Shared by the CLI `dep list` and MCP `dep`.
pub fn dep_list_value(conn: &Connection, id: &str) -> Result<Value> {
    let task = db::resolve_task(conn, id)?;
    let blockers = db::get_blockers(conn, &task.uuid)?;
    let blocking = db::get_blocking(conn, &task.uuid)?;
    let to_arr = |uuids: &[Uuid]| -> Vec<Value> {
        uuids
            .iter()
            .filter_map(|u| {
                db::get_task_by_uuid_prefix(conn, &u.to_string()[..8])
                    .ok()
                    .flatten()
                    .map(|t| json!({ "id": t.id, "uuid": t.uuid.to_string(), "description": t.description }))
            })
            .collect()
    };
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "description": task.description,
        "blocked_by": to_arr(&blockers),
        "blocking": to_arr(&blocking),
    }))
}

pub fn run_list(conn: &Connection, id: &str) -> Result<()> {
    let v = dep_list_value(conn, id)?;
    println!(
        "Task {}: {}",
        v["task"].as_i64().unwrap_or(0),
        v["description"].as_str().unwrap_or_default()
    );

    let print_group = |label: &str, arr: &Value| match arr.as_array() {
        Some(items) if !items.is_empty() => {
            println!("  {label}:");
            for t in items {
                println!(
                    "    {} — {}",
                    t["id"].as_i64().unwrap_or(0),
                    t["description"].as_str().unwrap_or_default()
                );
            }
        }
        _ => println!("  {label}: (none)"),
    };
    print_group("Blocked by", &v["blocked_by"]);
    print_group("Blocking", &v["blocking"]);
    Ok(())
}
