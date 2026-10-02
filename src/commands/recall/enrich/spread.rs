use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;
use std::collections::HashSet;

use crate::commands::recall::types::{Hit, Related};
use crate::infrastructure::db;

pub(in crate::commands::recall) const ASSOCIATIVE_CAP: usize = 5;

pub(in crate::commands::recall) const AUTO_SPREAD_HIT_FLOOR: usize = 3;

pub(in crate::commands::recall) fn should_auto_spread(hits: &[Hit]) -> bool {
    let memory_hits = hits.iter().filter(|h| h.item_uuid.is_some()).count();
    (1..AUTO_SPREAD_HIT_FLOOR).contains(&memory_hits)
}

pub(in crate::commands::recall) fn spreading_related(
    conn: &Connection,
    hits: &[Hit],
) -> Result<Vec<Related>> {
    let seeds: Vec<uuid::Uuid> = hits.iter().filter_map(|h| h.item_uuid).collect();
    if seeds.is_empty() {
        return Ok(vec![]);
    }
    let graph = crate::infrastructure::memory::graph::MemoryGraph::build(conn)?;
    if graph.is_empty() {
        return Ok(vec![]);
    }
    let seed_set: HashSet<uuid::Uuid> = seeds.iter().copied().collect();
    let mut out = vec![];
    for act in graph.spread_activation_explained(&seeds, 2, 0.6, 1e-6) {
        if seed_set.contains(&act.uuid) {
            continue;
        }
        if let Ok(item) = db::get_item_by_uuid(conn, &act.uuid.to_string()) {
            out.push(Related {
                item,
                activation: act.activation,
                path: act.path,
            });
        }
        if out.len() >= ASSOCIATIVE_CAP {
            break;
        }
    }
    let max = out.iter().map(|r| r.activation).fold(0.0_f64, f64::max);
    if max > 0.0 {
        for r in &mut out {
            r.activation /= max;
        }
    }
    Ok(out)
}

pub(in crate::commands::recall) fn associative_guide(
    conn: &Connection,
    related: &[Related],
) -> Vec<serde_json::Value> {
    related
        .iter()
        .map(|r| {
            let _ = db::record_memory_surfaced(conn, &r.item.uuid);
            json!({
                "label": format!("m{}", r.item.display_id.unwrap_or(0)),
                "preview": r.item.summary.clone().unwrap_or_else(|| r.item.body.clone()).chars().take(160).collect::<String>(),
                "activation": r.activation,
                "strength": db::item_strength(conn, &r.item),
                "via": r.path,
            })
        })
        .collect()
}
