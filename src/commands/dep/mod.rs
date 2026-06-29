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

pub fn run_off(conn: &Connection, cfg: &Config, id: &str, other: &str) -> Result<()> {
    let task = db::resolve_task(conn, id)?;
    let dep = db::resolve_task(conn, other)?;

    db::remove_dependency(conn, &task.uuid, &dep.uuid)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;
    db::refresh_urgency(conn, &cfg.urgency, &dep.uuid)?;

    println!(
        "Removed dependency: task {} no longer depends on task {}",
        task.id.unwrap_or(0),
        dep.id.unwrap_or(0),
    );
    Ok(())
}

pub fn run_list(conn: &Connection, id: &str) -> Result<()> {
    let task = db::resolve_task(conn, id)?;
    let blockers = db::get_blockers(conn, &task.uuid)?;
    let blocking = db::get_blocking(conn, &task.uuid)?;

    println!("Task {}: {}", task.id.unwrap_or(0), task.description);

    if blockers.is_empty() {
        println!("  Blocked by: (none)");
    } else {
        println!("  Blocked by:");
        for uuid in &blockers {
            if let Ok(Some(t)) = db::get_task_by_uuid_prefix(conn, &uuid.to_string()[..8]) {
                println!("    {} — {}", t.id.unwrap_or(0), t.description);
            }
        }
    }

    if blocking.is_empty() {
        println!("  Blocking: (none)");
    } else {
        println!("  Blocking:");
        for uuid in &blocking {
            if let Ok(Some(t)) = db::get_task_by_uuid_prefix(conn, &uuid.to_string()[..8]) {
                println!("    {} — {}", t.id.unwrap_or(0), t.description);
            }
        }
    }

    Ok(())
}
