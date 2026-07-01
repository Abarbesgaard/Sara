use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;

pub fn run_on(conn: &Connection, cfg: &Config, id: &str, other: &str) -> Result<()> {
    let task = db::resolve_task(conn, id)?;
    let dep = db::resolve_task(conn, other)?;

    db::add_dependency(conn, &task.uuid, &dep.uuid)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;
    db::refresh_urgency(conn, &cfg.urgency, &dep.uuid)?;

    println!(
        "Task {} now depends on task {} (\"{}\")",
        task.id.unwrap_or(0),
        dep.id.unwrap_or(0),
        dep.description
    );
    Ok(())
}
