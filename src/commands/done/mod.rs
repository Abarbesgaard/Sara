mod blockers;
mod recurrence;

use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Status;

/// Complete a task and return a structured record of what happened (including any
/// spawned recurrence). Print-free core shared by the CLI `done` command and the
/// MCP `done` tool. Errors if the task is blocked and `force` is false.
pub fn done_value(conn: &Connection, cfg: &Config, id_or_uuid: &str, force: bool) -> Result<Value> {
    let mut task = db::resolve_task(conn, id_or_uuid)?;

    blockers::ensure_not_blocked(conn, &task, force)?;

    // Finalize any running timer
    if let Some(started) = task.started_at {
        task.time_spent += (Utc::now() - started).num_seconds().max(0);
        task.started_at = None;
    }

    task.status = Status::Completed;
    task.end = Some(Utc::now());
    task.modified = Utc::now();
    db::update_task(conn, &task)?;

    // Repack display IDs
    db::repack_ids(conn)?;

    // Refresh urgency for tasks that were blocking on this one
    let was_blocking = db::get_blocking(conn, &task.uuid)?;
    for dep_uuid in was_blocking {
        let _ = db::refresh_urgency(conn, &cfg.urgency, &dep_uuid);
    }

    // Spawn next occurrence for recurring tasks
    let recurrence = recurrence::spawn_next(conn, cfg, &task)?;

    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "project": task.project,
        "description": task.description,
        "status": "completed",
        "recurrence": recurrence,
        "auto_memory": db::synthesize_done_memory(conn, &task.uuid, &task.project)
            .unwrap_or(None),
    }))
}

pub fn run(conn: &Connection, cfg: &Config, id_or_uuid: &str, force: bool) -> Result<()> {
    let v = done_value(conn, cfg, id_or_uuid, force)?;
    println!(
        "Done: [{}] {}",
        v["project"].as_str().unwrap_or_default(),
        v["description"].as_str().unwrap_or_default()
    );
    if let Some(rec) = v.get("recurrence").filter(|r| !r.is_null()) {
        println!(
            "♺  Next recurrence: #{} due {}",
            rec.get("id").and_then(|i| i.as_i64()).unwrap_or(0),
            rec.get("due").and_then(|d| d.as_str()).unwrap_or_default()
        );
    }
    if let Some(label) = v.get("auto_memory").and_then(|m| m.as_str()) {
        println!("🧠 Auto-memory saved: {label} (provisional — review with `sara memories`)");
    }
    Ok(())
}
