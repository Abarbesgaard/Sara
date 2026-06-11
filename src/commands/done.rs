use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

use crate::config::Config;
use crate::db;
use crate::model::Status;

pub fn run(conn: &Connection, cfg: &Config, id_or_uuid: &str, force: bool) -> Result<()> {
    let mut task = db::resolve_task(conn, id_or_uuid)?;

    // Check blockers
    let blockers = db::get_blockers(conn, &task.uuid)?;
    if !blockers.is_empty() && !force {
        let blocker_ids: Vec<String> = blockers
            .iter()
            .map(|u| {
                db::get_task_by_uuid_prefix(conn, &u.to_string()[..8])
                    .ok()
                    .flatten()
                    .and_then(|t| t.id)
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| u.to_string()[..8].to_string())
            })
            .collect();
        anyhow::bail!(
            "Task {} is blocked by tasks: {}. Use --force to complete anyway.",
            task.id.unwrap_or(0),
            blocker_ids.join(", ")
        );
    }

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

    println!(
        "Done: [{}] {}",
        task.project, task.description
    );

    // Refresh urgency for tasks that were blocking on this one
    let was_blocking = db::get_blocking(conn, &task.uuid)?;
    for dep_uuid in was_blocking {
        let _ = db::refresh_urgency(conn, &cfg.urgency, &dep_uuid);
    }

    Ok(())
}
