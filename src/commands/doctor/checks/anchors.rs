use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::commands::doctor::types::{Check, DETAIL_LIMIT, Labels, Status};
use crate::infrastructure::db;
use crate::infrastructure::memory::fingerprint::{self, AnchorState};

pub(in crate::commands::doctor) fn stale_anchor_check(
    conn: &Connection,
    labels: &Labels,
) -> Result<Vec<Check>> {
    let anchors = db::all_active_item_file_fingerprints(conn)?;
    let mut stale: Vec<(String, String, AnchorState)> = anchors
        .into_iter()
        .filter_map(|(uuid, file, fp)| {
            let state = fingerprint::anchor_state(&file, fp.as_deref());
            state.is_stale().then(|| (labels.of(&uuid), file, state))
        })
        .collect();
    stale.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let memories: std::collections::BTreeSet<&str> = stale.iter().map(|s| s.0.as_str()).collect();

    let mut c = Check::new(
        "stale_anchors",
        Status::Info,
        memories.len(),
        "sara relearn <label> <text> (re-validate) or sara forget <label>",
    );
    c.summary = if stale.is_empty() {
        "no anchored file has drifted or vanished since its memory was learned".to_string()
    } else {
        format!(
            "{} memory(ies) anchored to {} file(s) that drifted or vanished since learn",
            memories.len(),
            stale.len()
        )
    };
    c.details = json!(
        stale
            .iter()
            .take(DETAIL_LIMIT)
            .map(|(label, file, state)| match state {
                AnchorState::Drifted(d) => json!({
                    "label": label,
                    "file": file,
                    "reason": "drifted",
                    "drift": (d * 100.0).round() / 100.0,
                }),
                _ => json!({ "label": label, "file": file, "reason": "missing" }),
            })
            .collect::<Vec<_>>()
    );
    Ok(vec![c])
}
