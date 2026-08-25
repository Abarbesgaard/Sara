use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::HashSet;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::embedding::{self, Embedder};
use crate::infrastructure::model::Item;

/// Common English function words + conventional task prefixes that carry no
/// discriminating signal for similarity matching.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "in", "it", "of", "to", "for", "on", "at", "by", "up", "as", "or",
    "do", "if", "be", "we", "he", "she", "they", "but", "and", "not", "with", "from", "this",
    "that", "are", "was", "has", "have", "feat", "fix", "via", "add", "new",
];

/// Maximum number of tokens to AND together in a token-based search.
/// Too many required tokens produces zero results; cap at a useful sweet spot.
const MAX_AND_TOKENS: usize = 6;

/// Extract meaningful search tokens from free text: lowercase, alpha-only,
/// >=3 chars, not a stop word.
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

/// Emit a memory (learned item) as a similarity hit carrying its **full body** --
/// not a snippet -- so the agent receives the actual prior finding the instant
/// the charge is created, with no second `recall` round-trip. This is the core
/// of "relevant context, as early as possible": a pointer forces re-derivation,
/// the body prevents it.
fn memory_hit(item: &Item, confidence: &str, cosine: Option<f32>) -> Value {
    json!({
        "ref_kind": "memory",
        "confidence": confidence,
        "memory": item.display_id.map(|d| format!("m{d}")),
        "uuid": item.uuid.to_string(),
        "title": item.title,
        "tags": item.tags,
        "provisional": item.status == "provisional",
        "cosine": cosine,
        // Full, untruncated memory text -- the whole point of surfacing it here.
        "body": item.body,
    })
}

/// Best-effort recall of prior tasks AND learned memories whose content overlaps
/// with a new task -- run before creation so `add` surfaces "this may already be
/// solved" instead of an agent silently re-deriving an approach.
///
/// Four passes, most-precise first, deduped by uuid across passes:
///   Pass 0 -- **tag-exact memories** (canonical): any active memory carrying one
///            of the new task's tags. Highest confidence -- a deliberate topic hit.
///   Pass 1 -- **phrase match** (exact wording, any order): high confidence.
///   Pass 2 -- **token AND match** (stop-word-stripped tokens): medium confidence.
///   Pass 3 -- **semantic** (embedding cosine >= threshold): surfaces paraphrases
///            that share no literal keyword -- the fix that would have caught a
///            canonical memory whose wording differs from the new task's title.
///
/// Memory hits carry the full body; task hits stay a snippet + ref (a task is a
/// pointer to reopen, a memory is knowledge to apply now). Non-blocking: the
/// task is always created regardless of hits.
pub(super) fn find_similar(
    conn: &Connection,
    cfg: &Config,
    description: &str,
    tags: &[String],
    limit: i64,
) -> Result<Vec<Value>> {
    let mut seen_tasks = HashSet::new();
    let mut seen_mem: HashSet<String> = HashSet::new();
    let mut out: Vec<Value> = Vec::new();

    // -- Pass 0: tag-exact memories -> canonical confidence -------------------
    for tag in tags {
        let items = db::find_items_by_tag(conn, tag).unwrap_or_default();
        for item in &items {
            if !seen_mem.insert(item.uuid.to_string()) {
                continue;
            }
            out.push(memory_hit(item, "canonical", None));
        }
    }

    // -- Pass 1: phrase match -> high confidence ------------------------------
    let phrase_hits = db::search_fts(conn, description, limit).unwrap_or_default();
    for h in &phrase_hits {
        push_fts_hit(conn, h, "high", &mut seen_tasks, &mut seen_mem, &mut out);
    }

    // -- Pass 2: token AND match -> medium confidence -------------------------
    let tokens = meaningful_tokens(description);
    let capped: Vec<String> = tokens.into_iter().take(MAX_AND_TOKENS).collect();
    if !capped.is_empty() {
        let token_hits = db::search_fts_tokens(conn, &capped, limit).unwrap_or_default();
        for h in &token_hits {
            // Require at least half the query tokens to appear; otherwise the AND
            // match is too coincidental to be useful.
            if token_overlap(&capped, &h.text) < capped.len().div_ceil(2) {
                continue;
            }
            push_fts_hit(conn, h, "medium", &mut seen_tasks, &mut seen_mem, &mut out);
        }
    }

    // -- Pass 3: semantic memories -> confidence by cosine --------------------
    // Best-effort: any embed/storage hiccup leaves the lexical hits untouched.
    if let Err(e) = merge_semantic_memories(conn, cfg, description, &mut seen_mem, &mut out) {
        eprintln!("Warning: semantic recall on add failed: {e}");
    }

    Ok(out)
}

/// Resolve one FTS hit: memory hits (`item_*`) become full-body memory hits;
/// task/note/anchor hits become the existing snippet+ref pointer. The prior
/// implementation ran `resolve_task` on *every* hit, which silently dropped
/// memory hits (their `task_uuid` column carries the item's own uuid, not a
/// task) -- the bug that kept canonical memories from ever reaching `add`.
fn push_fts_hit(
    conn: &Connection,
    h: &db::SearchHit,
    confidence: &str,
    seen_tasks: &mut HashSet<String>,
    seen_mem: &mut HashSet<String>,
    out: &mut Vec<Value>,
) {
    if h.ref_kind.starts_with("item_") {
        // A learned memory: `task_uuid` carries the item's own uuid.
        if !seen_mem.insert(h.task_uuid.clone()) {
            return;
        }
        if let Ok(item) = db::get_item_by_uuid(conn, &h.task_uuid) {
            out.push(memory_hit(&item, confidence, None));
        }
        return;
    }

    // A task / annotation / anchor hit: a pointer to reopen, snippet is enough.
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

/// Rank stored memory embeddings against the new task's text and fold the
/// strongest (cosine >= configured threshold) in as full-body memory hits,
/// deduped against memories already surfaced lexically or by tag. This is what
/// catches a canonical memory whose wording differs from the new task's title.
fn merge_semantic_memories(
    conn: &Connection,
    cfg: &Config,
    query: &str,
    seen_mem: &mut HashSet<String>,
    out: &mut Vec<Value>,
) -> Result<()> {
    let qv = embedding::bundled().embed(query);
    if qv.iter().all(|&x| x == 0.0) {
        return Ok(()); // query had no in-vocabulary content
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
            continue; // already surfaced lexically or by tag
        }
        if let Ok(item) = db::get_item_by_uuid(conn, &uuid) {
            out.push(memory_hit(&item, "semantic", Some(cos)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::config::Config;
    use crate::infrastructure::model::{Item, Task};

    #[test]
    fn tag_exact_memory_surfaces_with_full_body_as_canonical() {
        let conn = db::open_in_memory_for_test();
        let cfg = Config::default();

        let long_body = "dependabot bumped NSubstitute 5.3.0->6.2.0; AutoFixture.AutoNSubstitute \
            4.18.1 caps it <6.0.0 -> NU1608. Revert NSubstitute to 5.3.0 across the test projects.";
        let mut mem = Item::new_memory("NSubstitute restore fault".into(), long_body.into(), None);
        mem.tags = vec!["nsubstitute".into()];
        mem.path = Some(String::new());
        db::insert_item(&conn, &mut mem).unwrap();
        db::set_item_tags(&conn, &mem.uuid, &["nsubstitute".into()]).unwrap();

        let hits = find_similar(
            &conn,
            &cfg,
            "restore fails on the dependabot bump",
            &["nsubstitute".to_string()],
            5,
        )
        .unwrap();

        let mem_hit = hits
            .iter()
            .find(|h| h["ref_kind"] == "memory")
            .expect("tag-exact memory must surface");
        assert_eq!(mem_hit["confidence"], "canonical");
        assert_eq!(
            mem_hit["body"].as_str().unwrap(),
            long_body,
            "the FULL body is emitted, not a snippet — no second recall needed"
        );
    }

    #[test]
    fn task_hits_still_surface_as_snippet_pointers() {
        let conn = db::open_in_memory_for_test();
        let cfg = Config::default();

        let mut prior = Task::new("fix the flaky payment retry logic".into(), "proj".into());
        db::insert_task(&conn, &mut prior).unwrap();

        let hits = find_similar(&conn, &cfg, "fix the flaky payment retry logic", &[], 5).unwrap();
        assert!(
            hits.iter().any(|h| h["ref_kind"] == "task"),
            "a matching prior task still surfaces as a pointer"
        );
    }
}
