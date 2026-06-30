use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;

pub fn link(conn: &Connection, id_or_uuid: &str, url: &str, label: Option<&str>) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    db::add_link(conn, &task.uuid, url, label)?;
    println!("Linked task {}: {}", task.id.unwrap_or(0), url);
    Ok(())
}

pub fn unlink(conn: &Connection, link_id: i64) -> Result<()> {
    if db::delete_link(conn, link_id)? {
        println!("Removed link {link_id}.");
    } else {
        anyhow::bail!("No link with id {link_id}");
    }
    Ok(())
}
