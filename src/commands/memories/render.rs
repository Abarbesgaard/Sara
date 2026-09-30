use std::collections::HashMap;

use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;
use uuid::Uuid;

use crate::commands::shared::{
    canonical_labels, derived_from_suffix, item_label, item_snippet, print_json, strength_label,
};
use crate::infrastructure::db;
use crate::infrastructure::model::Item;

pub(super) fn print_memories(
    conn: &Connection,
    memories: &[Item],
    strengths: &HashMap<Uuid, f64>,
    as_json: bool,
) -> Result<()> {
    if as_json {
        let v: Vec<_> = memories
            .iter()
            .map(|m| {
                let strength = strengths.get(&m.uuid).copied().unwrap_or(1.0);
                let label = item_label(m);
                let files = db::get_item_files(conn, &m.uuid).unwrap_or_default();
                let (derived_labels, derived_from_labels) = canonical_labels(conn, m);
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
                    "canonical": !derived_labels.is_empty(),
                    "derived_count": derived_labels.len(),
                    "derived_from": derived_from_labels,
                })
            })
            .collect();
        print_json(&json!({ "memories": v }))?;
        return Ok(());
    }

    if memories.is_empty() {
        println!("No memories recorded yet. Use `sara learn \"...\"` to save one.");
        return Ok(());
    }

    println!("Memories (newest first):");
    for m in memories {
        let strength = strengths.get(&m.uuid).copied().unwrap_or(1.0);
        let label = item_label(m);
        let tags_str = if m.tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", m.tags.join(", "))
        };
        let snippet = item_snippet(m, 100);
        let (derived_labels, derived_from_labels) = canonical_labels(conn, m);
        let canonical_str = if derived_labels.is_empty() {
            String::new()
        } else {
            format!(" [canonical, {} derived]", derived_labels.len())
        };
        let derived_from_str = derived_from_suffix(&derived_from_labels);
        println!(
            "  {} ({}){}{}{} {}{}: {}",
            label,
            strength_label(strength),
            if m.status == "provisional" {
                " [provisional]"
            } else {
                ""
            },
            canonical_str,
            derived_from_str,
            m.title,
            tags_str,
            snippet.trim()
        );
    }
    Ok(())
}
