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

const RELATED_THRESHOLD: f32 = 0.55;

const MAX_RELATED: usize = 2;

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

pub fn related_findings_json(related: &[Related]) -> Vec<Value> {
    related
        .iter()
        .map(|r| json!({ "annotation_id": r.id, "cosine": r.cosine, "text": r.text }))
        .collect()
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/insight/mod.rs"]
mod tests;
