use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;

pub fn run_chain(conn: &Connection, cfg: &Config, ids: &[String]) -> Result<()> {
    anyhow::ensure!(ids.len() >= 2, "dep chain requires at least 2 task ids");

    for pair in ids.windows(2) {
        let task = db::resolve_task(conn, &pair[0])?;
        let dep = db::resolve_task(conn, &pair[1])?;

        db::add_dependency(conn, &task.uuid, &dep.uuid)?;
        db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;
        db::refresh_urgency(conn, &cfg.urgency, &dep.uuid)?;

        println!(
            "Task {} now depends on task {} (\"{}\")",
            task.id.unwrap_or(0),
            dep.id.unwrap_or(0),
            dep.description
        );
    }
    Ok(())
}
