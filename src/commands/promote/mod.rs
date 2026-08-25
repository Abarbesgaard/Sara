use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

/// Print-free core shared by the CLI `promote` command and the MCP `promote`
/// tool. Marks a provisional (auto-synthesised) memory as reviewed by setting
/// status='active' in place — created date, tags, files, and task links are
/// all preserved (unlike the forget + re-learn workaround).
pub fn promote_value(conn: &Connection, handle: &str) -> Result<Value> {
    let item = db::get_item_by_handle(conn, handle)?;
    let promoted = db::promote_item(conn, &item.uuid)?;
    if !promoted {
        anyhow::bail!(
            "{handle} is not a provisional memory (status: {}) — nothing to promote.",
            item.status
        );
    }
    Ok(json!({
        "label": handle,
        "uuid": item.uuid.to_string(),
        "promoted": true,
    }))
}

/// `sara promote <label>` — promote a provisional auto-memory to active.
pub fn run(conn: &Connection, handle: &str) -> Result<()> {
    let v = promote_value(conn, handle)?;
    println!(
        "Promoted {}: now an active (reviewed) memory.",
        v["label"].as_str().unwrap_or(handle),
    );
    Ok(())
}
