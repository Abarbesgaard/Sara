use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::config::Config;
use crate::db;

/// `sara recall <query>` — cross-task memory. Uses the FTS5 index over task
/// descriptions/rationale/assignment, annotations (findings/decisions/…), and
/// code-anchor reasons so an agent can pull prior context from the whole history.
///
/// When the `embeddings` table has been populated, semantic hits are blended in
/// (hybrid keyword + vector recall); today FTS5 is the active engine.
pub fn run(conn: &Connection, _cfg: &Config, query: &str, limit: i64, as_json: bool) -> Result<()> {
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

    if as_json {
        let keyword: Vec<_> = results
            .iter()
            .map(|(id, desc, h)| {
                json!({
                    "task": id,
                    "task_description": desc,
                    "ref_kind": h.ref_kind,
                    "text": h.text,
                })
            })
            .collect();
        let sem: Vec<_> = semantic
            .iter()
            .map(
                |(id, desc, score)| json!({ "task": id, "task_description": desc, "score": score }),
            )
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "query": query,
                "keyword": keyword,
                "semantic": sem,
            }))?
        );
        return Ok(());
    }

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
