use anyhow::Result;
use rusqlite::Connection;

use crate::commands::recall::enrich::stale::mark_stale;
use crate::commands::recall::types::Hit;
use crate::commands::shared::{item_label, item_snippet, short_id};
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
    let uuid = item.uuid.to_string();
    let links_to = db::get_memory_links_to(conn, &uuid).unwrap_or_default();
    let links_from = db::get_memory_links_from(conn, &uuid).unwrap_or_default();
    let superseded_by = linked_labels(
        conn,
        links_to
            .iter()
            .filter(|l| l.relation == "supersedes")
            .map(|l| l.from_uuid.as_str()),
    );
    let derived_from_labels = linked_labels(
        conn,
        links_from
            .iter()
            .filter(|l| l.relation == "derived_from")
            .map(|l| l.to_uuid.as_str()),
    );
    let derived_children = linked_labels(
        conn,
        links_to
            .iter()
            .filter(|l| l.relation == "derived_from")
            .map(|l| l.from_uuid.as_str()),
    );
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

fn linked_labels<'a>(conn: &Connection, uuids: impl Iterator<Item = &'a str>) -> Vec<String> {
    uuids
        .map(|u| {
            db::get_item_by_uuid(conn, u)
                .ok()
                .map(|i| item_label(&i))
                .unwrap_or_else(|| short_id(u))
        })
        .collect()
}

pub(in crate::commands::recall) fn record_recalled(conn: &Connection, hits: &[Hit]) {
    for u in hits.iter().filter_map(|h| h.item_uuid) {
        let _ = db::record_memory_recall(conn, &u);
    }
}
