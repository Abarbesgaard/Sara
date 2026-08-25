use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

/// Core shared by CLI and MCP. Resolves memory labels (e.g. "m7") to UUIDs,
/// inserts the typed link, and returns structured output.
pub fn link_memory_value(
    conn: &Connection,
    from_handle: &str,
    relation: &str,
    to_handle: &str,
    weight: f64,
) -> Result<Value> {
    let from_item = db::get_item_by_handle(conn, from_handle)?;
    let to_item = db::get_item_by_handle(conn, to_handle)?;

    db::insert_memory_link(
        conn,
        &from_item.uuid.to_string(),
        &to_item.uuid.to_string(),
        relation,
        weight,
    )?;

    Ok(json!({
        "from": from_handle,
        "from_uuid": from_item.uuid.to_string(),
        "relation": relation,
        "to": to_handle,
        "to_uuid": to_item.uuid.to_string(),
        "weight": weight,
    }))
}

/// `sara link-memory <from> <relation> <to> [--weight N]`
///
/// Examples:
///   sara link-memory m12 supersedes m7
///   sara link-memory m3 similar_to m8
pub fn run(
    conn: &Connection,
    from_handle: &str,
    relation: &str,
    to_handle: &str,
    weight: f64,
) -> Result<()> {
    let v = link_memory_value(conn, from_handle, relation, to_handle, weight)?;
    println!(
        "Linked: {} {} {} (weight: {})",
        v["from"].as_str().unwrap_or(from_handle),
        v["relation"].as_str().unwrap_or(relation),
        v["to"].as_str().unwrap_or(to_handle),
        v["weight"].as_f64().unwrap_or(weight),
    );
    Ok(())
}

/// `sara unlink-memory <from> <relation> <to>` — remove a specific typed edge.
pub fn unlink(conn: &Connection, from_handle: &str, relation: &str, to_handle: &str) -> Result<()> {
    let v = unlink_value(conn, from_handle, relation, to_handle)?;
    let removed = v["removed"].as_bool().unwrap_or(false);
    if removed {
        println!("Unlinked: {from_handle} {relation} {to_handle}");
    } else {
        println!("No such link: {from_handle} {relation} {to_handle}");
    }
    Ok(())
}

/// Core shared by CLI and MCP for unlink.
pub fn unlink_value(
    conn: &Connection,
    from_handle: &str,
    relation: &str,
    to_handle: &str,
) -> Result<Value> {
    let from_item = db::get_item_by_handle(conn, from_handle)?;
    let to_item = db::get_item_by_handle(conn, to_handle)?;
    let removed = db::delete_memory_link(
        conn,
        &from_item.uuid.to_string(),
        &to_item.uuid.to_string(),
        relation,
    )?;
    Ok(json!({ "removed": removed, "from": from_handle, "relation": relation, "to": to_handle }))
}
