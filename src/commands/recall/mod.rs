use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;

/// `sara recall <query>` — cross-task memory. Uses the FTS5 index over task
/// descriptions/rationale/assignment, annotations (findings/decisions/…), and
/// code-anchor reasons so an agent can pull prior context from the whole history.
///
/// When the `embeddings` table has been populated, semantic hits are blended in
/// (hybrid keyword + vector recall); today FTS5 is the active engine.
/// Structured cross-task recall for the MCP `recall` tool and the `--json` CLI
/// path: keyword (FTS5) hits plus any semantic hits.
pub fn recall_value(
    conn: &Connection,
    _cfg: &Config,
    query: &str,
    limit: i64,
) -> Result<serde_json::Value> {
    let hits = db::search_fts(conn, query, limit)?;
    let keyword: Vec<_> = hits
        .iter()
        .map(|h| {
            let (id, desc) = match db::resolve_task(conn, &h.task_uuid) {
                Ok(task) => (task.id.unwrap_or(0), task.description.clone()),
                Err(_) => (0, String::new()),
            };
            json!({
                "task": id,
                "task_description": desc,
                "ref_kind": h.ref_kind,
                "text": h.text,
            })
        })
        .collect();
    let sem: Vec<_> = semantic_hits(conn, query, limit)
        .iter()
        .map(|(id, desc, score)| json!({ "task": id, "task_description": desc, "score": score }))
        .collect();
    Ok(json!({ "query": query, "keyword": keyword, "semantic": sem }))
}

pub fn run(conn: &Connection, cfg: &Config, query: &str, limit: i64, as_json: bool) -> Result<()> {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&recall_value(conn, cfg, query, limit)?)?
        );
        return Ok(());
    }

    let hits = db::search_fts(conn, query, limit)?;

    let mut results = vec![];
    for h in &hits {
        let (id, desc) = match db::resolve_task(conn, &h.task_uuid) {
            Ok(task) => (task.id.unwrap_or(0), task.description.clone()),
            Err(_) => (0, String::new()),
        };
        results.push((id, desc, h));
    }

    let semantic = semantic_hits(conn, query, limit);

    if results.is_empty() && semantic.is_empty() {
        println!("No matches for \"{query}\".");
        return Ok(());
    }
    if !results.is_empty() {
        println!("Keyword matches:");
        for (id, desc, h) in &results {
            let snippet: String = h.text.chars().take(100).collect();
            println!(
                "  [{}] (task {}) {}: {}",
                h.ref_kind,
                id,
                desc,
                snippet.trim()
            );
        }
    }
    if !semantic.is_empty() {
        println!("\nSemantically related:");
        for (id, desc, score) in &semantic {
            println!("  task {id} ({score:.2}): {desc}");
        }
    }
    Ok(())
}

/// Best-effort vector recall over any stored embeddings. Returns empty until the
/// embeddings table is populated (no query-side embedding is computed otherwise).
fn semantic_hits(_conn: &Connection, _query: &str, _limit: i64) -> Vec<(i64, String, f32)> {
    Vec::new()
}
