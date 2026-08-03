use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;

/// Default age in days before a Weak (no task link) memory is eligible for pruning.
pub const DEFAULT_WEAK_DAYS: i64 = 90;
/// Default age in days before a Provisional (auto-generated) memory is eligible
/// for pruning if never reviewed (promoted to active).
pub const DEFAULT_PROVISIONAL_DAYS: i64 = 30;

/// Core shared by CLI and MCP. Returns structured pruning report.
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

/// `sara prune-memories [--apply] [--weak-days N] [--provisional-days N]`
pub fn run(
    conn: &Connection,
    weak_days: i64,
    provisional_days: i64,
    dry_run: bool,
) -> Result<()> {
    let v = prune_value(conn, weak_days, provisional_days, dry_run)?;
    let count = v["archived"].as_u64().unwrap_or(0);
    let mode = if dry_run { "Would archive" } else { "Archived" };

    if count == 0 {
        println!("No low-value memories found.");
        return Ok(());
    }

    println!("{mode} {count} memories:");
    if let Some(memories) = v["memories"].as_array() {
        for m in memories {
            println!(
                "  {} {} — {}",
                m["label"].as_str().unwrap_or("?"),
                m["title"].as_str().unwrap_or(""),
                m["reason"].as_str().unwrap_or(""),
            );
        }
    }
    if dry_run {
        println!("\nRun with --apply to archive.");
    }
    Ok(())
}
