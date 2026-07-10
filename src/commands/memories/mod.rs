use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::infrastructure::db;

/// `sara memories` — browse all saved memories, newest first, with derived
/// strength shown so a human can audit which memories the recall system trusts.
pub fn run(conn: &Connection, as_json: bool) -> Result<()> {
    let memories = db::list_memories(conn)?;

    if as_json {
        let v: Vec<_> = memories
            .iter()
            .map(|m| {
                let strength = db::item_strength(conn, m);
                let label = format!(
                    "{}{}",
                    m.kind.chars().next().unwrap_or('m'),
                    m.display_id.unwrap_or(0)
                );
                let files = db::get_item_files(conn, &m.uuid).unwrap_or_default();
                json!({
                    "label": label,
                    "title": m.title,
                    "body": m.body,
                    "strength": strength,
                    "strength_label": strength_label(strength),
                    "provisional": m.status == "provisional",
                    "tags": m.tags,
                    "files": files,
                    "created": m.created.to_rfc3339(),
                    "modified": m.modified.to_rfc3339(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json!({ "memories": v }))?);
        return Ok(());
    }

    if memories.is_empty() {
        println!("No memories recorded yet. Use `sara learn \"...\"` to save one.");
        return Ok(());
    }

    println!("Memories (newest first):");
    for m in &memories {
        let strength = db::item_strength(conn, m);
        let label = format!(
            "{}{}",
            m.kind.chars().next().unwrap_or('m'),
            m.display_id.unwrap_or(0)
        );
        let tags_str = if m.tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", m.tags.join(", "))
        };
        let snippet: String = m
            .summary
            .as_deref()
            .unwrap_or(&m.body)
            .chars()
            .take(100)
            .collect();
        println!(
            "  {} ({}){} {}{}: {}",
            label,
            strength_label(strength),
            if m.status == "provisional" {
                " [provisional]"
            } else {
                ""
            },
            m.title,
            tags_str,
            snippet.trim()
        );
    }
    Ok(())
}

fn strength_label(s: f64) -> &'static str {
    if s >= 2.0 {
        "Strong"
    } else if s >= 1.5 {
        "Linked"
    } else {
        "Weak"
    }
}
