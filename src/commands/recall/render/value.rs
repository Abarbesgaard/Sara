use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::commands::recall::enrich::confidence::match_confidence;
use crate::commands::recall::enrich::hit::{item_hit, recent_hits, record_recalled};
use crate::commands::recall::enrich::patterns::detect_patterns;
use crate::commands::recall::enrich::provenance::mark_provenance;
use crate::commands::recall::enrich::spread::{
    associative_guide, should_auto_spread, spreading_related,
};
use crate::commands::recall::enrich::stale::mark_stale;
use crate::commands::recall::render::keyword::keyword_json;
use crate::commands::recall::search::collect::collect_hits;
use crate::commands::recall::search::query::{RecallInput, resolve_label_query};
use crate::commands::recall::search::semantic::SemanticOpts;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

pub fn recall_value(
    conn: &Connection,
    cfg: &Config,
    query: &str,
    tags: &[String],
    projects: &[String],
    files: &[String],
    limit: i64,
    spread: bool,
) -> Result<serde_json::Value> {
    let input = RecallInput::new(query, tags, projects, files);
    let recent = input.is_recent();
    let RecallInput {
        query,
        tags,
        projects,
        files,
    } = input;

    if recent {
        let hits = recent_hits(conn, limit)?;
        record_recalled(conn, &hits);
        let keyword = keyword_json(&hits);
        let patterns = detect_patterns(conn, &hits);
        return Ok(json!({
            "query": query,
            "tag": tags,
            "project": projects,
            "files": files,
            "keyword": keyword,
            "patterns": patterns,
            "associative": [],
            "confidence": "recent",
            "caveat": "Most recent memories (no query or filter given).",
            "recent": true,
        }));
    }

    if let Some(item) = resolve_label_query(conn, query) {
        let _ = db::record_memory_recall(conn, &item.uuid);
        let mut hit = item_hit(conn, item, true);
        mark_stale(conn, std::slice::from_mut(&mut hit));
        mark_provenance(conn, std::slice::from_mut(&mut hit));
        let related = spreading_related(conn, std::slice::from_ref(&hit))?;
        let associative = associative_guide(conn, &related);
        let label = hit.label.clone();
        return Ok(json!({
            "query": query,
            "tag": tags,
            "project": projects,
            "files": files,
            "keyword": keyword_json(std::slice::from_ref(&hit)),
            "associative": associative,
            "spread": "label",
            "confidence": "exact",
            "caveat": format!("Resolved memory {label} by label; `associative` maps its cluster."),
            "deep": true,
        }));
    }

    let hits = collect_hits(
        conn,
        query,
        &tags,
        &projects,
        &files,
        limit,
        &SemanticOpts::from_cfg(cfg),
    )?;
    let keyword = keyword_json(&hits);
    let patterns = detect_patterns(conn, &hits);

    let (confidence, caveat) = match_confidence(query, &tags, &hits);

    let auto_spread = !spread && !query.trim().is_empty() && should_auto_spread(&hits);
    let do_spread = spread || auto_spread;
    let associative: Vec<_> = if do_spread {
        let related = spreading_related(conn, &hits)?;
        associative_guide(conn, &related)
    } else {
        vec![]
    };

    Ok(json!({
        "query": query,
        "tag": tags,
        "project": projects,
        "files": files,
        "keyword": keyword,
        "patterns": patterns,
        "associative": associative,
        "spread": if spread { "explicit" } else if auto_spread { "auto" } else { "off" },
        "confidence": confidence,
        "caveat": caveat,
    }))
}
