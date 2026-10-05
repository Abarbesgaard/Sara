use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::commands::doctor::types::{Check, DETAIL_LIMIT, Labels, Status};
use crate::infrastructure::db;

pub(in crate::commands::doctor) fn orphan_check(
    conn: &Connection,
    labels: &Labels,
) -> Result<Vec<Check>> {
    let orphans = db::orphaned_memory_links(conn)?;
    let mut c = Check::new(
        "orphaned_links",
        Status::Warn,
        orphans.len(),
        "sara unlink-memory <from> <relation> <to>",
    );
    c.summary = if orphans.is_empty() {
        "every memory link points at a live memory".to_string()
    } else {
        format!(
            "{} memory link(s) point at an archived or missing memory",
            orphans.len()
        )
    };
    c.details = json!(
        orphans
            .iter()
            .take(DETAIL_LIMIT)
            .map(|l| json!({
                "from": labels.of(&l.from_uuid),
                "relation": l.relation,
                "to": labels.of(&l.to_uuid),
            }))
            .collect::<Vec<_>>()
    );
    Ok(vec![c])
}
