use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::commands::{diagnose_memories, prune_memories, reflect};
use crate::infrastructure::memory::{embedding, graph};

/// One memory-maintenance pass; every knob is optional and defaults to the
/// CLI's values.
#[derive(Debug, Default)]
pub struct MaintainRequest<'a> {
    pub action: &'a str,
    pub dry_run: Option<bool>,
    pub apply: Option<bool>,
    pub weak_days: Option<i64>,
    pub provisional_days: Option<i64>,
    pub window_days: Option<i64>,
    pub bucket_secs: Option<i64>,
    pub delta: Option<f64>,
    pub max_bucket: Option<usize>,
    pub min_weight: Option<f64>,
    pub max_cluster: Option<usize>,
    pub threshold: Option<f32>,
    pub project: Option<&'a str>,
    pub limit: Option<usize>,
}

pub const MAINTAIN_ACTIONS: [&str; 5] = ["diagnose", "consolidate", "reflect", "prune", "reindex"];

/// Run one memory-maintenance action and tag the result with it.
pub fn maintain_value(conn: &Connection, req: &MaintainRequest) -> Result<Value> {
    let result = match req.action {
        "diagnose" => diagnose_memories::diagnose_value(
            conn,
            req.threshold
                .unwrap_or(diagnose_memories::DEFAULT_CONFLICT_THRESHOLD),
            req.project,
            req.limit,
        )?,
        "consolidate" => {
            let reinforced = graph::consolidate(
                conn,
                req.window_days.unwrap_or(30),
                chrono::Duration::seconds(req.bucket_secs.unwrap_or(5)),
                req.delta.unwrap_or(0.1),
                req.max_bucket.unwrap_or(5),
            )?;
            json!({ "reinforced": reinforced })
        }
        "reflect" => {
            let min_weight = req.min_weight.unwrap_or(reflect::DEFAULT_MIN_WEIGHT);
            let max_cluster = req.max_cluster.unwrap_or(reflect::DEFAULT_MAX_CLUSTER);
            if req.apply.unwrap_or(false) {
                reflect::apply_value(conn, min_weight, max_cluster)?
            } else {
                reflect::reflect_value(conn, min_weight, max_cluster)?
            }
        }
        "prune" => prune_memories::prune_value(
            conn,
            req.weak_days.unwrap_or(prune_memories::DEFAULT_WEAK_DAYS),
            req.provisional_days
                .unwrap_or(prune_memories::DEFAULT_PROVISIONAL_DAYS),
            req.dry_run.unwrap_or(true),
        )?,
        "reindex" => json!({ "embedded": embedding::reindex_all(conn)? }),
        other => anyhow::bail!(
            "unknown maintain action `{other}` (expected one of: {})",
            MAINTAIN_ACTIONS.join(", ")
        ),
    };
    Ok(json!({ "action": req.action, "result": result }))
}
