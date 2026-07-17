use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::{self, Task};

/// Spawn the next occurrence for a recurring task, returning a JSON summary of
/// the new task (`id` + `due`). Returns `Value::Null` when the task does not
/// recur.
pub(super) fn spawn_next(conn: &Connection, cfg: &Config, task: &Task) -> Result<Value> {
    let Some(interval) = task.recur.clone() else {
        return Ok(Value::Null);
    };

    let base = task.due.unwrap_or_else(Utc::now);
    let next_due = model::advance_by_interval(base, &interval);
    let mut next = Task::new(task.description.clone(), task.project.clone());
    next.priority = task.priority.clone();
    next.tags = task.tags.clone();
    next.due = Some(next_due);
    next.recur = Some(interval.clone());
    next.estimate_mins = task.estimate_mins;
    next.urgency = db::compute_urgency(&next, &cfg.urgency, false, 0);
    db::insert_task(conn, &mut next)?;

    Ok(json!({
        "id": next.id,
        "due": next_due.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string(),
    }))
}
