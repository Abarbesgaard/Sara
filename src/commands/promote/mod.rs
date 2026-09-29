use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

mod render;

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

pub fn run(conn: &Connection, handle: &str) -> Result<()> {
    let v = promote_value(conn, handle)?;
    render::print_promoted(&v, handle);
    Ok(())
}
