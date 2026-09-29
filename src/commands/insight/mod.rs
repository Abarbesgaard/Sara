//! Surfacing a task's own prior findings that are semantically close to a new
//! result or finding — so an agent is reconnected to what it already concluded
//! the moment it records something that touches the same ground. This is the
//! guard against the "recorded a finding early, then contradicted it later
//! without noticing" failure: the earlier finding is pushed back into view (with
//! a reconsider prompt) exactly when the new text overlaps it in meaning.
//!
//! Deliberately embedding-based, not keyword-based: a contradiction rarely
//! shares surface wording ("no coupled NightOwl fault" vs "reverted NightOwl for
//! the coupled NU1605"), but it is semantically adjacent — which cosine catches
//! and FTS does not.

use rusqlite::Connection;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::infrastructure::db;
use crate::infrastructure::embedding::{self, Embedder};

mod render;
mod types;

#[allow(unused_imports)]
pub use render::print_related_findings;
pub use types::Related;

/// Cosine floor for calling a prior finding "related enough to reconsider".
/// Higher than the general recall threshold (0.30) because a false reconsider
/// prompt is noise the agent must burn a thought on — precision over recall.
const RELATED_THRESHOLD: f32 = 0.55;

/// At most this many prior findings are surfaced, strongest first — enough to
/// catch a contradiction without drowning the agent in its own backlog.
const MAX_RELATED: usize = 2;

/// Find the task's own prior `finding` annotations semantically closest to
/// `new_text`, above [`RELATED_THRESHOLD`], strongest first, capped at
/// [`MAX_RELATED`]. `exclude_id` skips the just-inserted annotation so a finding
/// never matches itself. Best-effort: returns empty on any embed hiccup.
pub fn related_findings(
    conn: &Connection,
    task_uuid: &Uuid,
    new_text: &str,
    exclude_id: Option<i64>,
) -> Vec<Related> {
    let qv = embedding::bundled().embed(new_text);
    if qv.iter().all(|&x| x == 0.0) {
        return Vec::new();
    }
    let Ok(anns) = db::get_annotations(conn, task_uuid) else {
        return Vec::new();
    };
    let mut scored: Vec<Related> = anns
        .into_iter()
        .filter(|a| a.kind == "finding" && Some(a.id) != exclude_id)
        .filter_map(|a| {
            let v = embedding::bundled().embed(&a.text);
            let c = embedding::cosine(&qv, &v);
            (c >= RELATED_THRESHOLD).then_some(Related {
                id: a.id,
                text: a.text,
                cosine: c,
            })
        })
        .collect();
    scored.sort_by(|a, b| {
        b.cosine
            .partial_cmp(&a.cosine)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(MAX_RELATED);
    scored
}

/// JSON form for MCP/`--json` callers.
pub fn related_findings_json(related: &[Related]) -> Vec<Value> {
    related
        .iter()
        .map(|r| json!({ "annotation_id": r.id, "cosine": r.cosine, "text": r.text }))
        .collect()
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/insight/mod.rs"]
mod tests;
