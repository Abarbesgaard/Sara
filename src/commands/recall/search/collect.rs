use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashSet;

use crate::commands::recall::enrich::clusters::collapse_clusters;
use crate::commands::recall::enrich::hit::item_hit;
use crate::commands::recall::enrich::stale::mark_stale;
use crate::commands::recall::search::filters::exact_uuids;
use crate::commands::recall::search::fts;
use crate::commands::recall::search::scoring;
use crate::commands::recall::search::semantic::{SemanticOpts, merge_semantic_hits};
use crate::commands::recall::types::Hit;
use crate::infrastructure::db;
use crate::infrastructure::model::Item;

pub(in crate::commands::recall) fn collect_hits(
    conn: &Connection,
    query: &str,
    tags: &[String],
    projects: &[String],
    files: &[String],
    limit: i64,
    semantic: &SemanticOpts,
) -> Result<Vec<Hit>> {
    let exact_uuids = exact_uuids(conn, tags, projects, files)?;

    let semantic_allowlist = exact_uuids.clone();
    let exact_items: Option<Vec<Item>> = if let Some(uuids) = exact_uuids {
        let mut items = vec![];
        for u in uuids {
            if let Ok(item) = db::get_item_by_uuid(conn, &u.to_string()) {
                items.push(item);
            }
        }
        Some(items)
    } else {
        None
    };

    let (fts_hits, loose) = fts::search(conn, query, limit)?;

    let mut hits = vec![];

    match exact_items {
        Some(items) => {
            let fts_item_uuids: HashSet<String> = fts_hits
                .iter()
                .filter(|h| h.ref_kind.starts_with("item_"))
                .map(|h| h.task_uuid.clone())
                .collect();
            for item in items {
                if !query.is_empty() && !fts_item_uuids.contains(&item.uuid.to_string()) {
                    continue;
                }
                hits.push(item_hit(conn, item, true));
            }
        }
        None => {
            for (rank, h) in fts_hits.iter().enumerate() {
                if h.ref_kind.starts_with("item_") {
                    if let Ok(item) = db::get_item_by_uuid(conn, &h.task_uuid) {
                        let mut hit = item_hit(conn, item, false);
                        hit.fts_rank = Some(rank);
                        hit.loose = loose;
                        hits.push(hit);
                    }
                    continue;
                }
                let (id, desc, modified) = match db::resolve_task(conn, &h.task_uuid) {
                    Ok(task) => (
                        task.id.unwrap_or(0),
                        task.description.clone(),
                        Some(task.modified),
                    ),
                    Err(_) => (0, String::new(), None),
                };
                hits.push(Hit {
                    ref_kind: h.ref_kind.clone(),
                    label: format!("task {id}"),
                    description: desc,
                    snippet: h.text.chars().take(160).collect(),
                    body: h.text.clone(),
                    strength: 1.0,
                    exact_match: false,
                    modified,
                    files: vec![],
                    linked_tasks: vec![],
                    superseded_by: vec![],
                    provisional: false,
                    item_uuid: None,
                    derived_from_labels: vec![],
                    derived_children: vec![],
                    fts_rank: Some(rank),
                    loose,
                    semantic: false,
                    cosine: None,
                    cluster: None,
                    stale: Vec::new(),
                });
            }
        }
    }

    if semantic.enabled && !query.is_empty() {
        merge_semantic_hits(
            conn,
            query,
            semantic,
            semantic_allowlist.as_ref(),
            &mut hits,
        )?;
    }

    let hits = scoring::rank(hits);

    let mut hits = collapse_clusters(conn, hits);
    hits.truncate(limit.max(0) as usize);
    mark_stale(conn, &mut hits);

    for h in &hits {
        if let Some(u) = h.item_uuid {
            let _ = db::record_memory_recall(conn, &u);
        }
    }

    Ok(hits)
}
