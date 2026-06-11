use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

use crate::config::Config;
use crate::db;
use crate::model::format_duration;

pub fn start(conn: &Connection, cfg: &Config, id_or_uuid: &str) -> Result<()> {
    let mut task = db::resolve_task(conn, id_or_uuid)?;

    if task.is_active() {
        println!(
            "Task {} is already active (running for {}).",
            task.id.unwrap_or(0),
            format_duration(task.total_time_spent())
        );
        return Ok(());
    }

    task.started_at = Some(Utc::now());
    task.modified = Utc::now();
    db::update_task(conn, &task)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;

    println!(
        "Started task {}: {}",
        task.id.unwrap_or(0),
        task.description
    );
    Ok(())
}

pub fn stop(conn: &Connection, cfg: &Config, id_or_uuid: &str) -> Result<()> {
    let mut task = db::resolve_task(conn, id_or_uuid)?;

    let Some(started) = task.started_at else {
        println!("Task {} is not active.", task.id.unwrap_or(0));
        return Ok(());
    };

    let session = (Utc::now() - started).num_seconds().max(0);
    task.time_spent += session;
    task.started_at = None;
    task.modified = Utc::now();
    db::update_task(conn, &task)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;

    println!(
        "Stopped task {} (this session: {}, total: {})",
        task.id.unwrap_or(0),
        format_duration(session),
        format_duration(task.time_spent)
    );
    Ok(())
}
