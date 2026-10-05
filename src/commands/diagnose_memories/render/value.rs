use serde_json::{Value, json};

use crate::commands::diagnose_memories::types::ConflictCandidate;

pub(in crate::commands::diagnose_memories) fn conflicts_json(
    mut candidates: Vec<ConflictCandidate>,
    threshold: f32,
    project: Option<&str>,
    limit: Option<usize>,
) -> Value {
    let total = candidates.len();
    if let Some(n) = limit {
        candidates.truncate(n);
    }
    let items: Vec<Value> = candidates
        .iter()
        .map(|c| {
            json!({
                "label_a":      c.label_a,
                "label_b":      c.label_b,
                "snippet_a":    c.snippet_a,
                "snippet_b":    c.snippet_b,
                "shared_files": c.shared_files,
                "shared_tags":  c.shared_tags,
                "cosine":       c.cosine,
            })
        })
        .collect();

    json!({
        "conflicts": items,
        "count": items.len(),
        "total": total,
        "threshold": threshold,
        "project": project,
    })
}
