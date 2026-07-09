use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

/// Common English function words + conventional task prefixes that carry no
/// discriminating signal for similarity matching.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "in", "it", "of", "to", "for", "on", "at",
    "by", "up", "as", "or", "do", "if", "be", "we", "he", "she", "they",
    "but", "and", "not", "with", "from", "this", "that", "are", "was",
    "has", "have", "feat", "fix", "via", "add", "new",
];

/// Maximum number of tokens to AND together in a token-based search.
/// Too many required tokens produces zero results; cap at a useful sweet spot.
const MAX_AND_TOKENS: usize = 6;

/// Extract meaningful search tokens from free text: lowercase, alpha-only,
/// ≥3 chars, not a stop word.
fn meaningful_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphabetic())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() >= 3 && !STOP_WORDS.contains(&w.as_str()))
        .collect()
}

/// Count how many of `query_tokens` appear (case-insensitive) in `text`.
fn token_overlap(query_tokens: &[String], text: &str) -> usize {
    let lower = text.to_lowercase();
    query_tokens
        .iter()
        .filter(|t| lower.contains(t.as_str()))
        .count()
}

/// Best-effort recall of prior tasks/memories whose description overlaps with a
/// new one — run before creation so `add` can surface "this may already be
/// solved" instead of an agent silently re-deriving an approach.
///
/// Two-pass strategy:
///   Pass 1 — phrase match (exact wording, any order → FTS5 string literal for
///             the full description): high confidence.
///   Pass 2 — token AND match (stop-word-stripped tokens, up to MAX_AND_TOKENS,
///             space-joined = FTS5 AND): medium confidence. Hits already seen in
///             pass 1 are skipped.
///
/// Non-blocking: the task is always created regardless of hits.
pub(super) fn find_similar(conn: &Connection, description: &str, limit: i64) -> Result<Vec<Value>> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<Value> = Vec::new();

    // ── Pass 1: phrase match → high confidence ───────────────────────────────
    let phrase_hits = db::search_fts(conn, description, limit).unwrap_or_default();
    for h in &phrase_hits {
        if !seen.insert(h.task_uuid.clone()) {
            continue;
        }
        let Ok(task) = db::resolve_task(conn, &h.task_uuid) else {
            continue;
        };
        let snippet: String = h.text.chars().take(160).collect();
        out.push(json!({
            "task": task.id.unwrap_or(0),
            "description": task.description,
            "ref_kind": h.ref_kind,
            "snippet": snippet.trim(),
            "confidence": "high",
        }));
    }

    // ── Pass 2: token AND match → medium confidence ──────────────────────────
    let tokens = meaningful_tokens(description);
    let capped: Vec<String> = tokens.into_iter().take(MAX_AND_TOKENS).collect();
    if !capped.is_empty() {
        let token_hits =
            db::search_fts_tokens(conn, &capped, limit).unwrap_or_default();
        for h in &token_hits {
            if !seen.insert(h.task_uuid.clone()) {
                continue; // already surfaced at high confidence
            }
            // Require at least half the query tokens to appear in the text;
            // otherwise the AND match is too coincidental to be useful.
            if token_overlap(&capped, &h.text) < (capped.len() + 1) / 2 {
                continue;
            }
            let Ok(task) = db::resolve_task(conn, &h.task_uuid) else {
                continue;
            };
            let snippet: String = h.text.chars().take(160).collect();
            out.push(json!({
                "task": task.id.unwrap_or(0),
                "description": task.description,
                "ref_kind": h.ref_kind,
                "snippet": snippet.trim(),
                "confidence": "medium",
            }));
        }
    }

    Ok(out)
}
