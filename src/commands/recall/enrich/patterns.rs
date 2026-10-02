use rusqlite::Connection;
use serde_json::json;

use crate::commands::recall::types::Hit;
use crate::commands::shared::{derived_children, item_label};
use crate::infrastructure::db;
use crate::infrastructure::model::Item;

pub(in crate::commands::recall) const PATTERN_MIN_INSTANCES: usize = 2;

pub(in crate::commands::recall) const PATTERN_TEXT_CAP: usize = 4000;

pub(in crate::commands::recall) fn derived_child_labels(
    conn: &Connection,
    uuid: &str,
) -> Vec<String> {
    derived_children(conn, uuid)
        .iter()
        .map(item_label)
        .collect()
}

pub(in crate::commands::recall) struct PatternAcc {
    item: Item,
    instances: Vec<String>,
    matched: Vec<String>,
}

pub(in crate::commands::recall) fn detect_patterns(
    conn: &Connection,
    hits: &[Hit],
) -> Vec<serde_json::Value> {
    use std::collections::BTreeMap;
    let mut anchors: BTreeMap<String, PatternAcc> = BTreeMap::new();

    for h in hits {
        let Some(uuid) = h.item_uuid else { continue };
        let mut candidates: Vec<String> = Vec::new();
        if !h.derived_children.is_empty() {
            candidates.push(uuid.to_string());
        }
        for l in db::get_memory_links_from(conn, &uuid.to_string()).unwrap_or_default() {
            if l.relation == "derived_from" {
                candidates.push(l.to_uuid);
            }
        }

        for anchor_uuid in candidates {
            let entry = anchors.entry(anchor_uuid.clone());
            let acc = match entry {
                std::collections::btree_map::Entry::Occupied(o) => o.into_mut(),
                std::collections::btree_map::Entry::Vacant(v) => {
                    let Ok(item) = db::get_item_by_uuid(conn, &anchor_uuid) else {
                        continue;
                    };
                    let instances = derived_child_labels(conn, &anchor_uuid);
                    v.insert(PatternAcc {
                        item,
                        instances,
                        matched: Vec::new(),
                    })
                }
            };
            if !acc.matched.contains(&h.label) {
                acc.matched.push(h.label.clone());
            }
        }
    }

    let mut patterns: Vec<(usize, usize, serde_json::Value)> = anchors
        .into_values()
        .filter(|a| a.instances.len() >= PATTERN_MIN_INSTANCES)
        .map(|a| {
            let label = item_label(&a.item);
            let occurrences = a.instances.len();
            let title = a.item.title.trim().to_string();
            let text: String = a.item.body.chars().take(PATTERN_TEXT_CAP).collect();
            let strength = db::item_strength(conn, &a.item);
            let guide = format!(
                "Recurring pattern: '{label}' has been applied {occurrences} times before \
                 (see `instances`). The canonical `text` above is the proven approach — \
                 rather than re-deriving it, create a task with `add` using it as the \
                 step-by-step guide, then `learn` the outcome and link that memory \
                 `derived_from {label}` so the pattern keeps strengthening."
            );
            let matched = a.matched.clone();
            let v = json!({
                "canonical": label,
                "title": title,
                "text": text,
                "strength": strength,
                "occurrences": occurrences,
                "instances": a.instances,
                "matched_hits": matched,
                "guide": guide,
            });
            (occurrences, a.matched.len(), v)
        })
        .collect();

    patterns.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    patterns.into_iter().map(|(_, _, v)| v).collect()
}
