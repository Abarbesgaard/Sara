use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashSet;

use crate::commands::recall::enrich::hit::item_hit;
use crate::commands::recall::types::Hit;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

pub(in crate::commands::recall) struct SemanticOpts {
    pub(in crate::commands::recall) enabled: bool,
    pub(in crate::commands::recall) threshold: f32,
    pub(in crate::commands::recall) top_k: usize,
}

impl SemanticOpts {
    pub(in crate::commands::recall) fn from_cfg(cfg: &Config) -> Self {
        SemanticOpts {
            enabled: true,
            threshold: cfg.recall.semantic_threshold,
            top_k: cfg.recall.semantic_top_k,
        }
    }

    pub(in crate::commands::recall) fn off() -> Self {
        SemanticOpts {
            enabled: false,
            threshold: 1.0,
            top_k: 0,
        }
    }
}

pub(in crate::commands::recall) fn merge_semantic_hits(
    conn: &Connection,
    query: &str,
    opts: &SemanticOpts,
    allowlist: Option<&HashSet<uuid::Uuid>>,
    hits: &mut Vec<Hit>,
) -> Result<()> {
    use crate::infrastructure::memory::embedding::{self, Embedder};

    let qv = embedding::bundled().embed(query);
    if qv.iter().all(|&x| x == 0.0) {
        return Ok(());
    }

    let already: HashSet<String> = hits
        .iter()
        .filter_map(|h| h.item_uuid.map(|u| u.to_string()))
        .collect();

    let mut scored: Vec<(String, f32)> = db::active_embeddings(conn)?
        .into_iter()
        .filter(|(uuid, _)| {
            if already.contains(uuid) {
                return false;
            }
            match allowlist {
                Some(allow) => matches!(uuid::Uuid::parse_str(uuid), Ok(u) if allow.contains(&u)),
                None => true,
            }
        })
        .map(|(uuid, v)| (uuid, embedding::cosine(&qv, &v)))
        .filter(|(_, c)| *c >= opts.threshold)
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(opts.top_k);

    for (uuid, cos) in scored {
        if let Ok(item) = db::get_item_by_uuid(conn, &uuid) {
            let mut hit = item_hit(conn, item, false);
            hit.semantic = true;
            hit.cosine = Some(cos);
            hits.push(hit);
        }
    }
    Ok(())
}
