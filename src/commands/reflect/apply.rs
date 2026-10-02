use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use super::propose::reflect_value;
use crate::infrastructure::db;

pub fn apply_value(conn: &Connection, min_weight: f64, max_cluster: usize) -> Result<Value> {
    let proposal = reflect_value(conn, min_weight, max_cluster)?;
    let clusters = proposal["clusters"].as_array().cloned().unwrap_or_default();

    let mut applied: Vec<Value> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();

    for c in &clusters {
        let canonical = c["suggested_canonical"].as_str().unwrap_or_default();
        for link in c["proposed_links"].as_array().into_iter().flatten() {
            let from = link["from"].as_str().unwrap_or_default();
            let (from_uuid, to_uuid) = match (
                db::get_item_by_handle(conn, from),
                db::get_item_by_handle(conn, canonical),
            ) {
                (Ok(f), Ok(t)) => (f.uuid.to_string(), t.uuid.to_string()),
                _ => {
                    skipped.push(json!({
                        "from": from,
                        "to": canonical,
                        "relation": "derived_from",
                        "reason": "could not resolve memory label",
                    }));
                    continue;
                }
            };
            match db::insert_memory_link(conn, &from_uuid, &to_uuid, "derived_from", 1.0) {
                Ok(()) => applied.push(json!({
                    "from": from,
                    "relation": "derived_from",
                    "to": canonical,
                })),
                Err(e) => skipped.push(json!({
                    "from": from,
                    "to": canonical,
                    "relation": "derived_from",
                    "reason": e.to_string(),
                })),
            }
        }
    }

    Ok(json!({
        "applied": applied.len(),
        "links": applied,
        "skipped": skipped,
        "clusters": clusters.len(),
    }))
}
