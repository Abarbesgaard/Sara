use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::commands::doctor::types::{Check, DETAIL_LIMIT, Status};
use crate::infrastructure::db;

const REASON_SUPERSEDED: &str = "superseded by a newer memory";
const REASON_PROVISIONAL: &str = "provisional auto-memory not reviewed within time limit";

pub(in crate::commands::doctor) fn superseded_and_backlog_checks(
    conn: &Connection,
) -> Result<Vec<Check>> {
    let candidates = db::prune_memories(
        conn,
        db::AUTO_HYGIENE_WEAK_DAYS,
        db::AUTO_HYGIENE_PROVISIONAL_DAYS,
        true,
    )?;
    let labels_for = |reason: &str| -> Vec<String> {
        candidates
            .iter()
            .filter(|c| c.reason == reason)
            .map(|c| c.label.clone())
            .collect()
    };

    let superseded = labels_for(REASON_SUPERSEDED);
    let mut s = Check::new(
        "superseded",
        Status::Warn,
        superseded.len(),
        "sara prune-memories --apply",
    );
    s.summary = if superseded.is_empty() {
        "no superseded memory is still active".to_string()
    } else {
        format!(
            "{} superseded memory(ies) still active and competing in recall",
            superseded.len()
        )
    };
    s.details = json!(superseded.iter().take(DETAIL_LIMIT).collect::<Vec<_>>());

    let backlog = labels_for(REASON_PROVISIONAL);
    let mut b = Check::new(
        "provisional_backlog",
        Status::Warn,
        backlog.len(),
        "sara promote <label> (keep) or sara prune-memories --apply (archive)",
    );
    b.summary = if backlog.is_empty() {
        format!(
            "no provisional memory older than {} days awaits review",
            db::AUTO_HYGIENE_PROVISIONAL_DAYS
        )
    } else {
        format!(
            "{} provisional memory(ies) older than {} days await promote or forget",
            backlog.len(),
            db::AUTO_HYGIENE_PROVISIONAL_DAYS
        )
    };
    b.details = json!(backlog.iter().take(DETAIL_LIMIT).collect::<Vec<_>>());
    Ok(vec![s, b])
}
