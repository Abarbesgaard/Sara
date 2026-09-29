use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::HashSet;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::memory::embedding::{self, Embedder};
use crate::infrastructure::model::Item;

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "in", "it", "of", "to", "for", "on", "at", "by", "up", "as", "or",
    "do", "if", "be", "we", "he", "she", "they", "but", "and", "not", "with", "from", "this",
    "that", "are", "was", "has", "have", "feat", "fix", "via", "add", "new",
];

const MAX_AND_TOKENS: usize = 6;

fn meaningful_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphabetic())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() >= 3 && !STOP_WORDS.contains(&w.as_str()))
        .collect()
}

fn token_overlap(query_tokens: &[String], text: &str) -> usize {
    let lower = text.to_lowercase();
    query_tokens
        .iter()
        .filter(|t| lower.contains(t.as_str()))
        .count()
}

fn memory_hit(item: &Item, confidence: &str, cosine: Option<f32>, current_project: &str) -> Value {
    let same_project = item.project.as_deref() == Some(current_project);
    json!({
        "ref_kind": "memory",
        "confidence": confidence,
        "memory": item.display_id.map(|d| format!("m{d}")),
        "uuid": item.uuid.to_string(),
        "title": item.title,
        "tags": item.tags,
        "provisional": item.status == "provisional",
        "cosine": cosine,
        "project": item.project,
        "same_project": same_project,
        "body": item.body,
    })
}

pub(super) fn find_similar(
    conn: &Connection,
    cfg: &Config,
    description: &str,
    tags: &[String],
    current_project: &str,
    limit: i64,
) -> Result<Vec<Value>> {
    let mut seen_tasks = HashSet::new();
    let mut seen_mem: HashSet<String> = HashSet::new();
    let mut out: Vec<Value> = Vec::new();

    for tag in tags {
        let items = db::find_items_by_tag(conn, tag).unwrap_or_default();
        for item in &items {
            if !seen_mem.insert(item.uuid.to_string()) {
                continue;
            }
            out.push(memory_hit(item, "canonical", None, current_project));
        }
    }

    let phrase_hits = db::search_fts(conn, description, limit).unwrap_or_default();
    for h in &phrase_hits {
        push_fts_hit(
            conn,
            h,
            "high",
            current_project,
            &mut seen_tasks,
            &mut seen_mem,
            &mut out,
        );
    }

    let tokens = meaningful_tokens(description);
    let capped: Vec<String> = tokens.into_iter().take(MAX_AND_TOKENS).collect();
    if !capped.is_empty() {
        let token_hits = db::search_fts_tokens(conn, &capped, limit).unwrap_or_default();
        for h in &token_hits {
            if token_overlap(&capped, &h.text) < capped.len().div_ceil(2) {
                continue;
            }
            push_fts_hit(
                conn,
                h,
                "medium",
                current_project,
                &mut seen_tasks,
                &mut seen_mem,
                &mut out,
            );
        }
    }

    if let Err(e) = merge_semantic_memories(
        conn,
        cfg,
        description,
        current_project,
        &mut seen_mem,
        &mut out,
    ) {
        eprintln!("Warning: semantic recall on add failed: {e}");
    }

    out.sort_by_key(|h| {
        !h.get("same_project")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    });

    Ok(out)
}

fn push_fts_hit(
    conn: &Connection,
    h: &db::SearchHit,
    confidence: &str,
    current_project: &str,
    seen_tasks: &mut HashSet<String>,
    seen_mem: &mut HashSet<String>,
    out: &mut Vec<Value>,
) {
    if h.ref_kind.starts_with("item_") {
        if !seen_mem.insert(h.task_uuid.clone()) {
            return;
        }
        if let Ok(item) = db::get_item_by_uuid(conn, &h.task_uuid) {
            out.push(memory_hit(&item, confidence, None, current_project));
        }
        return;
    }

    if !seen_tasks.insert(h.task_uuid.clone()) {
        return;
    }
    let Ok(task) = db::resolve_task(conn, &h.task_uuid) else {
        return;
    };
    let snippet: String = h.text.chars().take(160).collect();
    out.push(json!({
        "ref_kind": h.ref_kind,
        "confidence": confidence,
        "task": task.id.unwrap_or(0),
        "description": task.description,
        "snippet": snippet.trim(),
    }));
}

fn merge_semantic_memories(
    conn: &Connection,
    cfg: &Config,
    query: &str,
    current_project: &str,
    seen_mem: &mut HashSet<String>,
    out: &mut Vec<Value>,
) -> Result<()> {
    let qv = embedding::bundled().embed(query);
    if qv.iter().all(|&x| x == 0.0) {
        return Ok(());
    }
    let threshold = cfg.recall.semantic_threshold;
    let top_k = cfg.recall.semantic_top_k;

    let mut scored: Vec<(String, f32)> = db::active_embeddings(conn)?
        .into_iter()
        .map(|(uuid, v)| (uuid, embedding::cosine(&qv, &v)))
        .filter(|(_, c)| *c >= threshold)
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);

    for (uuid, cos) in scored {
        if !seen_mem.insert(uuid.clone()) {
            continue;
        }
        if let Ok(item) = db::get_item_by_uuid(conn, &uuid) {
            out.push(memory_hit(&item, "semantic", Some(cos), current_project));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/add/similar.rs"]
mod tests;
