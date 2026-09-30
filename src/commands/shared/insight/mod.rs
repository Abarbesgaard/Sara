use rusqlite::Connection;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::infrastructure::db;
use crate::infrastructure::memory::embedding::{self, Embedder};

mod types;

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

pub fn print_related_json(v: &Value, with_hint: bool) {
    let Some(related) = v.get("related_findings").and_then(|r| r.as_array()) else {
        return;
    };
    if related.is_empty() {
        return;
    }
    eprintln!("⟳ reconsider — related prior finding(s) on this task:");
    for r in related {
        eprintln!(
            "    (~{:.2}) #{}: {}",
            r.get("cosine").and_then(|c| c.as_f64()).unwrap_or(0.0),
            r.get("annotation_id").and_then(|i| i.as_i64()).unwrap_or(0),
            r.get("text").and_then(|t| t.as_str()).unwrap_or("")
        );
    }
    if with_hint {
        eprintln!(
            "  If your new note revises or contradicts one, correct it (denotate / re-annotate)."
        );
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/commands/shared/insight.rs"]
mod tests;
