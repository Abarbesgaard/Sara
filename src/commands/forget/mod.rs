use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::io::{self, Write};

use crate::infrastructure::db;

/// Print-free core shared by the CLI `forget` command and the MCP `forget` tool.
pub fn forget_value(conn: &Connection, handle: &str) -> Result<Value> {
    let item = db::get_item_by_handle(conn, handle)?;
    db::archive_item(conn, &item.uuid)?;
    Ok(json!({
        "label": handle,
        "uuid": item.uuid.to_string(),
        "archived": true,
    }))
}

/// `sara forget <label>` — archive a memory by its label (e.g. m3).
pub fn run(conn: &Connection, handle: &str, yes: bool) -> Result<()> {
    if !yes {
        let item = db::get_item_by_handle(conn, handle)?;
        let title = item.title.trim();
        let preview = if title.is_empty() {
            item.summary.as_deref().unwrap_or("").trim()
        } else {
            title
        };
        print!("Forget {handle} \"{preview}\"? [y/N]: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }
    let v = forget_value(conn, handle)?;
    println!(
        "Forgot {}: archived.",
        v["label"].as_str().unwrap_or(handle),
    );
    Ok(())
}
