use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::commands::doctor::types::{Check, DETAIL_LIMIT, Status};

pub(in crate::commands::doctor) fn duplicate_check(conn: &Connection) -> Result<Vec<Check>> {
    let diag = crate::commands::diagnose_memories::diagnose_value(
        conn,
        crate::commands::diagnose_memories::DEFAULT_CONFLICT_THRESHOLD,
        None,
        Some(DETAIL_LIMIT),
    )?;
    let total = diag["total"].as_u64().unwrap_or(0) as usize;
    let mut c = Check::new("duplicates", Status::Warn, total, "sara diagnose-memories");
    c.summary = if total == 0 {
        "no near-duplicate or contradictory memory pairs".to_string()
    } else {
        format!("{total} unlinked memory pair(s) look like duplicates or contradictions")
    };
    c.details = json!(
        diag["conflicts"]
            .as_array()
            .map(|pairs| pairs
                .iter()
                .map(|p| json!({ "a": p["label_a"], "b": p["label_b"], "cosine": p["cosine"] }))
                .collect::<Vec<_>>())
            .unwrap_or_default()
    );
    Ok(vec![c])
}
