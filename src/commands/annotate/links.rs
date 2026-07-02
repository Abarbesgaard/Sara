use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

/// Attach a URL to a task and return a structured record. Print-free core shared
/// by the CLI `link` command and the MCP `link` tool.
pub fn link_value(
    conn: &Connection,
    id_or_uuid: &str,
    url: &str,
    label: Option<&str>,
) -> Result<Value> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    db::add_link(conn, &task.uuid, url, label)?;
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "url": url,
        "label": label,
    }))
}

pub fn link(conn: &Connection, id_or_uuid: &str, url: &str, label: Option<&str>) -> Result<()> {
    let v = link_value(conn, id_or_uuid, url, label)?;
    println!("Linked task {}: {}", v["task"].as_i64().unwrap_or(0), url);
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
