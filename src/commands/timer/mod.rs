use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::git;
use crate::infrastructure::model::format_duration;

/// Start the timer for a task, returning a structured record (no-op if already
/// active). Print-free core shared by the CLI `start` command and MCP `start`.
pub fn start_value(conn: &Connection, cfg: &Config, id_or_uuid: &str) -> Result<Value> {
    let mut task = db::resolve_task(conn, id_or_uuid)?;

    if task.is_active() {
        return Ok(json!({
            "task": task.id,
            "uuid": task.uuid.to_string(),
            "started": false,
            "already_active": true,
            "elapsed_seconds": task.total_time_spent(),
        }));
    }

    task.started_at = Some(Utc::now());
    task.modified = Utc::now();
    db::update_task(conn, &task)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;

    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "started": true,
        "description": task.description,
    }))
}

pub fn start(conn: &Connection, cfg: &Config, id_or_uuid: &str) -> Result<()> {
    let v = start_value(conn, cfg, id_or_uuid)?;
    if v["already_active"].as_bool().unwrap_or(false) {
        println!(
            "Task {} is already active (running for {}).",
            v["task"].as_i64().unwrap_or(0),
            format_duration(v["elapsed_seconds"].as_i64().unwrap_or(0))
        );
    } else {
        println!(
            "Started task {}: {}",
            v["task"].as_i64().unwrap_or(0),
            v["description"].as_str().unwrap_or_default()
        );
    }
    Ok(())
}

/// Stop the timer for a task (and snapshot a tied branch's changed files, if any),
/// returning a structured record. Print-free core shared by the CLI `stop` command
/// and the MCP `stop` tool. Branch-snapshot warnings still go to stderr (safe on
/// the stdio transport, which uses stdout as the JSON-RPC channel).
pub fn stop_value(conn: &Connection, cfg: &Config, id_or_uuid: &str) -> Result<Value> {
    let mut task = db::resolve_task(conn, id_or_uuid)?;

    let Some(started) = task.started_at else {
        return Ok(json!({
            "task": task.id,
            "uuid": task.uuid.to_string(),
            "stopped": false,
            "active": false,
        }));
    };

    let session = (Utc::now() - started).num_seconds().max(0);
    task.time_spent += session;
    task.started_at = None;
    task.modified = Utc::now();
    db::update_task(conn, &task)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;

    // If this task has a tied branch, snapshot its changed files.
    let mut branch_log = Value::Null;
    if let Some(branch_rec) = db::get_task_branch(conn, &task.uuid) {
        let project_path = db::get_project(conn, &task.project)
            .ok()
            .flatten()
            .and_then(|p| p.path);

        if let Some(path) = project_path {
            let repo = std::path::Path::new(&path);
            match git::changed_files(repo, &branch_rec.branch) {
                Ok((base, files)) => {
                    let n = files.len();
                    let _ = db::log_branch_changes(conn, &task.uuid, &base, &files);
                    branch_log = json!({ "branch": branch_rec.branch, "files_logged": n });
                }
                Err(e) => {
                    eprintln!("Warning: could not snapshot branch changes: {e:#}");
                }
            }
        }
    }

    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "stopped": true,
        "session_seconds": session,
        "total_seconds": task.time_spent,
        "branch_log": branch_log,
    }))
}

pub fn stop(conn: &Connection, cfg: &Config, id_or_uuid: &str) -> Result<()> {
    let v = stop_value(conn, cfg, id_or_uuid)?;

    if !v["stopped"].as_bool().unwrap_or(false) {
        println!("Task {} is not active.", v["task"].as_i64().unwrap_or(0));
        return Ok(());
    }

    println!(
        "Stopped task {} (this session: {}, total: {})",
        v["task"].as_i64().unwrap_or(0),
        format_duration(v["session_seconds"].as_i64().unwrap_or(0)),
        format_duration(v["total_seconds"].as_i64().unwrap_or(0))
    );

    if let Some(bl) = v.get("branch_log").filter(|b| !b.is_null()) {
        let n = bl["files_logged"].as_i64().unwrap_or(0);
        println!(
            "Logged {} changed file{} on branch '{}'.",
            n,
            if n == 1 { "" } else { "s" },
            bl["branch"].as_str().unwrap_or_default()
        );
    }

    Ok(())
}
