use anyhow::Result;
use rusqlite::Connection;
use uuid::Uuid;

use crate::commands::shared::{derived_children, memory_handle};
use crate::infrastructure::db;

const LINK_WEIGHT: f64 = 1.0;

pub(super) fn link_handles(
    conn: &Connection,
    new_uuid: &str,
    handles: &[String],
    relation: &str,
    flag: &str,
) -> Result<Vec<String>> {
    let mut labels = Vec::new();
    for handle in handles {
        let target = match db::get_item_by_handle(conn, handle) {
            Ok(target) => target,
            Err(e) => {
                eprintln!("warning: could not resolve memory '{handle}' for {flag}: {e}");
                continue;
            }
        };
        let target_uuid = target.uuid.to_string();
        db::insert_memory_link(conn, new_uuid, &target_uuid, relation, LINK_WEIGHT)?;
        let label = target
            .display_id
            .map(|id| format!("m{id}"))
            .unwrap_or_else(|| handle.clone());
        if relation == "supersedes" {
            warn_superseded_canonical(conn, &label, &target_uuid);
        }
        labels.push(label);
    }
    Ok(labels)
}

fn warn_superseded_canonical(conn: &Connection, label: &str, uuid: &str) {
    let derived: Vec<String> = derived_children(conn, uuid)
        .iter()
        .map(memory_handle)
        .collect();
    if derived.is_empty() {
        return;
    }
    eprintln!(
        "Warning: {label} is a canonical pattern memory with {} derived {} ({}) — review with \
         `sara dream <label>` or archive with `sara forget <label>`; they are not auto-archived \
         by this supersede.",
        derived.len(),
        if derived.len() == 1 {
            "memory"
        } else {
            "memories"
        },
        derived.join(", ")
    );
}

pub(super) fn auto_link_canonical(
    conn: &Connection,
    new_uuid: &str,
    candidates: &[Uuid],
    already_linked: &[&String],
) -> Result<Vec<String>> {
    let mut labels: Vec<String> = Vec::new();
    for u in candidates {
        let Ok(target) = db::get_item_by_uuid(conn, &u.to_string()) else {
            continue;
        };
        let label = memory_handle(&target);
        if already_linked.contains(&&label) || labels.contains(&label) {
            continue;
        }
        db::insert_memory_link(conn, new_uuid, &u.to_string(), "derived_from", LINK_WEIGHT)?;
        labels.push(label);
    }
    Ok(labels)
}
