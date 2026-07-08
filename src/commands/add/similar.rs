use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

/// Best-effort recall of prior tasks whose description/notes overlap with a new
/// one, run before creation so `add` can surface "this may already be solved"
/// instead of an agent silently re-deriving an approach. Non-blocking: results
/// are informational only, the task is created either way (no confidence
/// scoring or gating yet — see Sara task #43).
pub(super) fn find_similar(conn: &Connection, description: &str, limit: i64) -> Result<Vec<Value>> {
    let hits = db::search_fts(conn, description, limit)?;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for h in hits {
        if !seen.insert(h.task_uuid.clone()) {
            continue;
        }
        let Ok(task) = db::resolve_task(conn, &h.task_uuid) else {
            continue;
        };
        let snippet: String = h.text.chars().take(160).collect();
        out.push(json!({
            "task": task.id.unwrap_or(0),
            "description": task.description,
            "ref_kind": h.ref_kind,
            "snippet": snippet.trim(),
        }));
    }
    Ok(out)
}
