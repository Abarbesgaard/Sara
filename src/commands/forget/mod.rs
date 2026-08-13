use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

/// Print-free core shared by the CLI `forget` command and the MCP `forget` tool.
pub fn forget_value(conn: &Connection, handle: &str) -> Result<Value> {
    let item = db::get_item_by_handle(conn, handle)?;
    db::archive_item(conn, &item.uuid)?;
    // Drop any semantic-index entry too, so a forgotten memory can never
    // resurface via `recall --semantic`.
    let _ = db::delete_embedding(conn, &item.uuid.to_string());
    Ok(json!({
        "label": handle,
        "uuid": item.uuid.to_string(),
        "archived": true,
    }))
}

/// `sara forget <label>` — archive a memory by its label (e.g. m3).
pub fn run(conn: &Connection, handle: &str) -> Result<()> {
    let v = forget_value(conn, handle)?;
    println!(
        "Forgot {}: archived.",
        v["label"].as_str().unwrap_or(handle),
    );
    Ok(())
}
