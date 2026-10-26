use std::collections::HashSet;

use rusqlite::Connection;
use serde_json::{Value, json};
use uuid::Uuid;

use super::files::file_overlaps;
use super::tags::TagOverlap;
use crate::commands::shared::{item_snippet, memory_handle};
use crate::infrastructure::db;
use crate::infrastructure::memory::embedding;

/// Cosine similarity at which a memory's text reads as a near-duplicate.
const SIMILAR_TEXT: f32 = 0.8;
const MAX_REPORTED: usize = 5;

struct Candidate {
    uuid: Uuid,
    reasons: Vec<&'static str>,
    similarity: Option<f32>,
}

/// Memories the new one may duplicate, as structured data an agent can act on
/// (the CLI prints the same signals as warnings on stderr, which MCP clients
/// never see). Each entry names a typed link to fix it with.
pub(in crate::commands::learn) fn possible_duplicates(
    conn: &Connection,
    new_uuid: &Uuid,
    overlap: &TagOverlap,
    files: &[String],
    linked: &[&String],
) -> Vec<Value> {
    let mut found: Vec<Candidate> = Vec::new();
    let mut note = |uuid: Uuid, reason: &'static str, similarity: Option<f32>| {
        if uuid == *new_uuid {
            return;
        }
        match found.iter_mut().find(|c| c.uuid == uuid) {
            Some(c) => {
                if !c.reasons.contains(&reason) {
                    c.reasons.push(reason);
                }
                c.similarity = c.similarity.or(similarity);
            }
            None => found.push(Candidate {
                uuid,
                reasons: vec![reason],
                similarity,
            }),
        }
    };

    for u in &overlap.near_dupes {
        note(*u, "same_tags", None);
    }
    if let Ok(shared) = file_overlaps(conn, files, &HashSet::new()) {
        for (_, u) in shared {
            note(u, "shared_file", None);
        }
    }
    if let Ok(Some(mine)) = db::get_embedding(conn, &new_uuid.to_string()) {
        for (uuid, v) in db::active_embeddings(conn).unwrap_or_default() {
            let sim = embedding::cosine(&mine, &v);
            if sim >= SIMILAR_TEXT
                && let Ok(u) = Uuid::parse_str(&uuid)
            {
                note(u, "similar_text", Some(sim));
            }
        }
    }
    for u in &overlap.partial {
        note(*u, "partial_tag_overlap", None);
    }

    let rank = |c: &Candidate| -> (usize, i64) {
        let strong = c
            .reasons
            .iter()
            .filter(|r| **r != "partial_tag_overlap")
            .count();
        (strong, (c.similarity.unwrap_or(0.0) * 1000.0) as i64)
    };
    found.sort_by_key(|c| std::cmp::Reverse(rank(c)));

    found
        .into_iter()
        .filter_map(|c| {
            let item = db::get_item_by_uuid(conn, &c.uuid.to_string()).ok()?;
            if item.kind != "memory" {
                return None;
            }
            let label = memory_handle(&item);
            if linked.iter().any(|l| **l == label) {
                return None;
            }
            let suggestion = if c.reasons == ["partial_tag_overlap"] {
                format!("link_memory from=<new> relation=similar_to to={label} if related")
            } else {
                format!(
                    "if it replaces {label}: link_memory from=<new> relation=supersedes to={label}; \
                     if it specialises it: relation=derived_from; if it says the same: forget <new> and relearn {label}"
                )
            };
            Some(json!({
                "label": label,
                "preview": item_snippet(&item, 120),
                "reasons": c.reasons,
                "similarity": c.similarity.map(|s| (s * 100.0).round() / 100.0),
                "suggestion": suggestion,
            }))
        })
        .take(MAX_REPORTED)
        .collect()
}
