use anyhow::Result;
use rusqlite::Connection;

use crate::commands::recall::enrich::stale::mark_stale;
use crate::commands::recall::types::Hit;
use crate::commands::shared::{item_label, item_snippet};
use crate::infrastructure::db;
use crate::infrastructure::model::Item;

pub(in crate::commands::recall) fn recent_hits(conn: &Connection, limit: i64) -> Result<Vec<Hit>> {
    let mut memories = db::list_memories(conn)?;
    memories.truncate(limit.max(0) as usize);
    let mut hits: Vec<Hit> = memories
        .into_iter()
        .map(|m| item_hit(conn, m, false))
        .collect();
    mark_stale(conn, &mut hits);
    Ok(hits)
}

pub(in crate::commands::recall) fn item_hit(
    conn: &Connection,
    item: Item,
    exact_match: bool,
) -> Hit {
    let handle = item_label(&item);
    let snippet = item_snippet(&item, 160);
    let files = db::get_item_files(conn, &item.uuid).unwrap_or_default();
    let linked_tasks = db::get_item_task_links(conn, &item.uuid).unwrap_or_default();
    let superseded_by: Vec<String> = db::get_memory_links_to(conn, &item.uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "supersedes")
        .map(|l| {
            db::get_item_by_uuid(conn, &l.from_uuid)
                .ok()
                .map(|i| item_label(&i))
                .unwrap_or_else(|| l.from_uuid.chars().take(8).collect::<String>())
        })
        .collect();
    let derived_from_labels: Vec<String> = db::get_memory_links_from(conn, &item.uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .map(|l| {
            db::get_item_by_uuid(conn, &l.to_uuid)
                .ok()
                .map(|i| item_label(&i))
                .unwrap_or_else(|| l.to_uuid.chars().take(8).collect::<String>())
        })
        .collect();
    let derived_children: Vec<String> = db::get_memory_links_to(conn, &item.uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .map(|l| {
            db::get_item_by_uuid(conn, &l.from_uuid)
                .ok()
                .map(|i| item_label(&i))
                .unwrap_or_else(|| l.from_uuid.chars().take(8).collect::<String>())
        })
        .collect();
    Hit {
        ref_kind: format!("item_{}", item.kind),
        strength: db::item_strength(conn, &item),
        label: handle,
        description: item.title.clone(),
        snippet,
        body: item.body.clone(),
        exact_match,
        modified: Some(item.modified),
        files,
        linked_tasks,
        superseded_by,
        provisional: item.status == "provisional",
        item_uuid: Some(item.uuid),
        derived_from_labels,
        derived_children,
        fts_rank: None,
        loose: false,
        semantic: false,
        cosine: None,
        cluster: None,
        stale: Vec::new(),
    }
}
