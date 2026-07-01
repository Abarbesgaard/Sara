use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};
use uuid::Uuid;

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

pub fn run_off(conn: &Connection, cfg: &Config, id: &str, other: &str) -> Result<()> {
    let v = dep_off_value(conn, cfg, id, other)?;
    println!(
        "Removed dependency: task {} no longer depends on task {}",
        v["task"].as_i64().unwrap_or(0),
        v["removed"]["id"].as_i64().unwrap_or(0),
    );
    Ok(())
}

pub fn run_chain(conn: &Connection, cfg: &Config, ids: &[String]) -> Result<()> {
    anyhow::ensure!(ids.len() >= 2, "dep chain requires at least 2 task ids");
    for pair in ids.windows(2) {
        run_on(conn, cfg, &pair[0], &pair[1])?;
    }
    Ok(())
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
