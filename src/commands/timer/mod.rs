use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::git;

mod render;

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
    render::print_started(&v);
    Ok(())
}

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
    render::print_stopped(&v);
    Ok(())
}
