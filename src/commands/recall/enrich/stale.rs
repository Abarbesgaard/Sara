use rusqlite::Connection;
use serde_json::json;

use crate::commands::recall::types::Hit;
use crate::infrastructure::db;
use crate::infrastructure::memory::fingerprint::{self, AnchorState};

/// Judge each hit's anchored files against the fingerprints taken when the
/// memory was learned; only the final, truncated hits are checked.
pub(in crate::commands::recall) fn mark_stale(conn: &Connection, hits: &mut [Hit]) {
    for h in hits.iter_mut() {
        let Some(u) = h.item_uuid else { continue };
        if h.files.is_empty() {
            continue;
        }
        h.stale = db::get_item_file_fingerprints(conn, &u)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(file, fp)| {
                let state = fingerprint::anchor_state(&file, fp.as_deref());
                state.is_stale().then_some((file, state))
            })
            .collect();
    }
}

pub(in crate::commands::recall) fn stale_json(h: &Hit) -> Vec<serde_json::Value> {
    h.stale
        .iter()
        .map(|(file, state)| match state {
            AnchorState::Drifted(d) => {
                json!({ "file": file, "reason": "drifted", "drift": (d * 100.0).round() / 100.0 })
            }
            _ => json!({ "file": file, "reason": "missing" }),
        })
        .collect()
}

pub(in crate::commands::recall) fn stale_text(h: &Hit) -> String {
    h.stale
        .iter()
        .map(|(file, state)| match state {
            AnchorState::Drifted(d) => format!(" {file} ({:.0}% changed)", d * 100.0),
            _ => format!(" {file} (missing)"),
        })
        .collect::<Vec<_>>()
        .join(",")
}
