use serde_json::json;

use crate::commands::recall::enrich::stale::stale_json;
use crate::commands::recall::types::Hit;

pub(in crate::commands::recall) fn keyword_json(hits: &[Hit]) -> Vec<serde_json::Value> {
    hits.iter()
        .enumerate()
        .map(|(i, h)| {
            if i == 0 {
                json!({
                    "ref_kind": h.ref_kind,
                    "label": h.label,
                    "description": h.description,
                    "preview": h.snippet,
                    "text": h.body,
                    "strength": h.strength,
                    "exact_match": h.exact_match,
                    "loose": h.loose,
                    "semantic": h.semantic,
                    "cosine": h.cosine,
                    "modified": h.modified.map(|m| m.to_rfc3339()),
                    "files": h.files,
                    "superseded_by": h.superseded_by,
                    "provisional": h.provisional,
                    "canonical": !h.derived_children.is_empty(),
                    "derived_count": h.derived_children.len(),
                    "derived_children": h.derived_children,
                    "derived_from": h.derived_from_labels,
                    "cluster": h.cluster.as_ref().map(|c| json!({
                        "canonical": c.canonical_label,
                        "size": c.size,
                        "collapsed_here": c.collapsed_here,
                        "nearest": c.nearest,
                    })),
                    "stale": stale_json(h),
                    "linked_tasks": h.linked_tasks.iter().map(|(t, src)| json!({
                        "id": t.id.unwrap_or(0),
                        "description": t.description,
                        "source": src,
                    })).collect::<Vec<_>>(),
                })
            } else {
                keyword_guide(h)
            }
        })
        .collect()
}

pub(in crate::commands::recall) fn keyword_guide(h: &Hit) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("label".into(), json!(h.label));
    map.insert("preview".into(), json!(h.snippet));
    map.insert("strength".into(), json!(h.strength));
    if !h.derived_children.is_empty() {
        map.insert("canonical".into(), json!(true));
    }
    if !h.derived_from_labels.is_empty() {
        map.insert("derived_from".into(), json!(h.derived_from_labels));
    }
    if let Some(c) = h.cluster.as_ref() {
        let mut cluster = json!({
            "canonical": c.canonical_label,
            "size": c.size,
            "collapsed_here": c.collapsed_here,
        });
        if let Some(n) = &c.nearest {
            cluster["nearest"] = json!(n);
        }
        map.insert("cluster".into(), cluster);
    }
    if !h.superseded_by.is_empty() {
        map.insert("superseded_by".into(), json!(h.superseded_by));
    }
    if !h.stale.is_empty() {
        map.insert("stale".into(), json!(stale_json(h)));
    }
    if !h.linked_tasks.is_empty() {
        map.insert(
            "linked_tasks".into(),
            json!(
                h.linked_tasks
                    .iter()
                    .map(|(t, _src)| t.id.unwrap_or(0))
                    .collect::<Vec<_>>()
            ),
        );
    }
    serde_json::Value::Object(map)
}
