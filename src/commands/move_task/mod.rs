use anyhow::{Result, bail};
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;

mod render;

pub fn move_value(conn: &Connection, cfg: &Config, id: &str, project: &str) -> Result<Value> {
    let target = project.trim();
    if target.is_empty() {
        bail!("Target project name cannot be empty");
    }
    let mut task = db::resolve_task(conn, id)?;
    let from = task.project.clone();
    let display_id = task.id.unwrap_or(0);

    if from == target {
        return Ok(json!({ "task": display_id, "from": from, "to": target, "changed": false }));
    }

    task.project = target.to_string();
    task.modified = chrono::Utc::now();
    db::update_task(conn, &task)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;

    Ok(json!({ "task": display_id, "from": from, "to": target, "changed": true }))
}

pub fn run(conn: &Connection, cfg: &Config, id: &str, project: &str) -> Result<()> {
    let v = move_value(conn, cfg, id, project)?;
    render::print_moved(&v, project);
    Ok(())
}
