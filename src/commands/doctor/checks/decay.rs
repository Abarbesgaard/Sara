use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde_json::json;
use std::collections::HashMap;

use crate::commands::doctor::types::{Check, DETAIL_LIMIT, Labels, Status};
use crate::infrastructure::db;
use crate::infrastructure::model::Item;

/// Recall events older than this are pruned, so decay is judged within it.
const RECALL_WINDOW_DAYS: i64 = 90;
const OVER_REINFORCED_MIN_RECALLS: usize = 20;
const OVER_REINFORCED_MEAN_FACTOR: f64 = 5.0;

pub(in crate::commands::doctor) fn decay_check(
    conn: &Connection,
    memories: &[Item],
    labels: &Labels,
) -> Result<Vec<Check>> {
    let now = Utc::now();
    let cutoff = now - chrono::Duration::days(RECALL_WINDOW_DAYS);
    let mut recalls: HashMap<uuid::Uuid, usize> = HashMap::new();
    for (uuid, _) in db::memory_recall_events_since(conn, &cutoff)? {
        *recalls.entry(uuid).or_default() += 1;
    }

    let dead: Vec<String> = memories
        .iter()
        .filter(|m| m.created <= cutoff && !recalls.contains_key(&m.uuid))
        .map(|m| labels.of(&m.uuid.to_string()))
        .collect();

    let recalled: Vec<(&uuid::Uuid, usize)> = recalls
        .iter()
        .filter(|(u, _)| memories.iter().any(|m| &m.uuid == *u))
        .map(|(u, n)| (u, *n))
        .collect();
    let mean = if recalled.is_empty() {
        0.0
    } else {
        recalled.iter().map(|(_, n)| *n as f64).sum::<f64>() / recalled.len() as f64
    };
    let mut hot: Vec<(String, usize)> = recalled
        .iter()
        .filter(|(_, n)| {
            *n >= OVER_REINFORCED_MIN_RECALLS && *n as f64 >= OVER_REINFORCED_MEAN_FACTOR * mean
        })
        .map(|(u, n)| (labels.of(&u.to_string()), *n))
        .collect();
    hot.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut c = Check::new(
        "decay_outliers",
        Status::Info,
        dead.len() + hot.len(),
        "sara forget <label> (dead weight) or sara relearn <label> (split an over-reinforced memory)",
    );
    c.summary = format!(
        "{} memory(ies) not recalled in {RECALL_WINDOW_DAYS} days, {} over-reinforced",
        dead.len(),
        hot.len()
    );
    c.details = json!({
        "window_days": RECALL_WINDOW_DAYS,
        "dead_weight": dead.iter().take(DETAIL_LIMIT).collect::<Vec<_>>(),
        "over_reinforced": hot
            .iter()
            .take(DETAIL_LIMIT)
            .map(|(l, n)| json!({ "label": l, "recalls": n }))
            .collect::<Vec<_>>(),
    });
    Ok(vec![c])
}
