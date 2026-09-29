use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

mod render;

pub fn forget_value(conn: &Connection, handle: &str, cascade: bool) -> Result<Value> {
    let item = db::get_item_by_handle(conn, handle)?;
    let derived: Vec<(String, uuid::Uuid)> = db::get_memory_links_to(conn, &item.uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .filter_map(|l| {
            db::get_item_by_uuid(conn, &l.from_uuid)
                .ok()
                .map(|i| (format!("m{}", i.display_id.unwrap_or(0)), i.uuid))
        })
        .collect();

    db::archive_item(conn, &item.uuid)?;
    let _ = db::delete_embedding(conn, &item.uuid.to_string());

    let mut cascaded: Vec<String> = Vec::new();
    if cascade {
        for (label, uuid) in &derived {
            if db::archive_item(conn, uuid).is_ok() {
                let _ = db::delete_embedding(conn, &uuid.to_string());
                cascaded.push(label.clone());
            }
        }
    }

    Ok(json!({
        "label": handle,
        "uuid": item.uuid.to_string(),
        "archived": true,
        "derived": derived.iter().map(|(l, _)| l.clone()).collect::<Vec<_>>(),
        "cascaded": cascaded,
    }))
}

pub fn run(conn: &Connection, handle: &str, cascade: bool) -> Result<()> {
    let v = forget_value(conn, handle, cascade)?;
    render::print_forgotten(&v, handle, cascade);
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/forget/mod.rs"]
mod tests;
