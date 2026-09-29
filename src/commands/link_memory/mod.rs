use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

mod render;

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

pub fn run(
    conn: &Connection,
    from_handle: &str,
    relation: &str,
    to_handle: &str,
    weight: f64,
) -> Result<()> {
    let v = link_memory_value(conn, from_handle, relation, to_handle, weight)?;
    render::print_linked(&v, from_handle, relation, to_handle, weight);
    Ok(())
}

pub fn unlink(conn: &Connection, from_handle: &str, relation: &str, to_handle: &str) -> Result<()> {
    let v = unlink_value(conn, from_handle, relation, to_handle)?;
    render::print_unlinked(&v, from_handle, relation, to_handle);
    Ok(())
}

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
