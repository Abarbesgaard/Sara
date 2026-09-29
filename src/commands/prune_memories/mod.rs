use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

mod render;

pub const DEFAULT_WEAK_DAYS: i64 = 90;
pub const DEFAULT_PROVISIONAL_DAYS: i64 = 30;

pub fn prune_value(
    conn: &Connection,
    weak_days: i64,
    provisional_days: i64,
    dry_run: bool,
) -> Result<Value> {
    let candidates = db::prune_memories(conn, weak_days, provisional_days, dry_run)?;
    let items: Vec<Value> = candidates
        .iter()
        .map(|c| {
            json!({
                "label": c.label,
                "uuid": &c.uuid[..8],
                "title": c.title,
                "reason": c.reason,
            })
        })
        .collect();
    Ok(json!({
        "dry_run": dry_run,
        "archived": items.len(),
        "memories": items,
        "weak_days": weak_days,
        "provisional_days": provisional_days,
    }))
}

pub fn run(conn: &Connection, weak_days: i64, provisional_days: i64, dry_run: bool) -> Result<()> {
    let v = prune_value(conn, weak_days, provisional_days, dry_run)?;
    render::print_pruned(&v, dry_run);
    Ok(())
}
