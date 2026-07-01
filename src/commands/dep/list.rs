use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;

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
