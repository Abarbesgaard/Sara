use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use std::io::{self, Write};

use crate::infrastructure::db;
use crate::infrastructure::model::Status;

pub fn run(conn: &Connection, id_or_uuid: &str, yes: bool) -> Result<()> {
    let mut task = db::resolve_task(conn, id_or_uuid)?;

    if !yes {
        print!(
            "Delete task {} \"{}\"? [y/N]: ",
            task.id.unwrap_or(0),
            task.description
        );
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    // Warn about tasks that depended on this one
    let was_blocking = db::get_blocking(conn, &task.uuid)?;
    if !was_blocking.is_empty() {
        eprintln!(
            "Warning: {} task(s) depended on this task and will be unblocked.",
            was_blocking.len()
        );
    }

    task.status = Status::Deleted;
    task.end = Some(Utc::now());
    task.modified = Utc::now();
    db::update_task(conn, &task)?;
    db::repack_ids(conn)?;

    println!("Deleted: {}", task.description);
    Ok(())
}
