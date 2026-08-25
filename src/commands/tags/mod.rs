use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::infrastructure::db;

/// `sara tags` — list all distinct memory tags with usage counts, most-used
/// first. Intended to be run before `sara learn` to discover the existing
/// vocabulary and avoid near-duplicate tags (service-a vs serviceA).
pub fn run(conn: &Connection, as_json: bool) -> Result<()> {
    let tags = db::list_tags_with_counts(conn)?;

    if as_json {
        let v: Vec<_> = tags
            .iter()
            .map(|(tag, count)| json!({ "tag": tag, "count": count }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&json!({ "tags": v }))?);
        return Ok(());
    }

    if tags.is_empty() {
        println!("No tags recorded yet. Use `sara learn --tag <topic> \"...\"` to tag a memory.");
        return Ok(());
    }

    println!("Memory tags (most used first):");
    for (tag, count) in &tags {
        println!("  {:>4}  {tag}", count);
    }
    Ok(())
}
