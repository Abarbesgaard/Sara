use anyhow::{Result, bail};
use rusqlite::Connection;

use crate::config::Config;
use crate::db;

/// Move a task to another project (non-interactive reassignment).
///
/// Resolves the task by display id or uuid prefix, sets its project, records the
/// change in history (via `update_task`), and refreshes its urgency since the
/// `project` component may change.
pub fn run(conn: &Connection, cfg: &Config, id: &str, project: &str) -> Result<()> {
    let target = project.trim();
    if target.is_empty() {
        bail!("Target project name cannot be empty");
    }

    let mut task = db::resolve_task(conn, id)?;
    let from = task.project.clone();
    let display_id = task.id.unwrap_or(0);

    if from == target {
        println!("Task {display_id} is already in project '{target}'.");
        return Ok(());
    }

    task.project = target.to_string();
    task.modified = chrono::Utc::now();
    db::update_task(conn, &task)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;

    println!("Moved task {display_id} to project '{target}' (was '{from}').");
    Ok(())
}
