use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

use crate::infrastructure::db;
use crate::infrastructure::model::Status;

mod render;

pub fn run(conn: &Connection, id_or_uuid: &str, yes: bool) -> Result<()> {
    let mut task = db::resolve_task(conn, id_or_uuid)?;

    if !yes && !render::confirm_delete(task.id.unwrap_or(0), &task.description)? {
        render::print_cancelled();
        return Ok(());
    }

    let was_blocking = db::get_blocking(conn, &task.uuid)?;
    if !was_blocking.is_empty() {
        render::warn_unblocked(was_blocking.len());
    }

    task.status = Status::Deleted;
    task.end = Some(Utc::now());
    task.modified = Utc::now();
    db::update_task(conn, &task)?;
    db::repack_ids(conn)?;

    render::print_deleted(&task.description);
    Ok(())
}
