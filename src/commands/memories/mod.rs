use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::infrastructure::db;

/// `sara memories` — browse all saved memories, newest first, with derived
/// strength shown so a human can audit which memories the recall system trusts.
pub fn run(conn: &Connection, as_json: bool) -> Result<()> {
    let memories = db::list_memories(conn)?;
    // One grouped query per strength component instead of one per memory:
    // `item_strength` scans the `memory_recalled` event log every call, so the
    // per-item form made this command O(memories x recall events).
    let strengths = db::item_strengths(conn, &memories);

    if as_json {
        let v: Vec<_> = memories
            .iter()
            .map(|m| {
                let strength = strengths.get(&m.uuid).copied().unwrap_or(1.0);
                let label = format!(
                    "{}{}",
                    m.kind.chars().next().unwrap_or('m'),
                    m.display_id.unwrap_or(0)
                );
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
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "memories": v }))?
        );
        return Ok(());
    }

    if memories.is_empty() {
        println!("No memories recorded yet. Use `sara learn \"...\"` to save one.");
        return Ok(());
    }

    println!("Memories (newest first):");
    for m in &memories {
        let strength = strengths.get(&m.uuid).copied().unwrap_or(1.0);
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
        let (derived_labels, derived_from_labels) = canonical_labels(conn, m);
        let canonical_str = if derived_labels.is_empty() {
            String::new()
        } else {
            format!(" [canonical, {} derived]", derived_labels.len())
        };
        let derived_from_str = if derived_from_labels.is_empty() {
            String::new()
        } else {
            format!(" [derived from: {}]", derived_from_labels.join(", "))
        };
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

/// Resolve a memory's canonical/derived labels: `(labels of derived children
/// via incoming derived_from edges, labels of canonicals via outgoing
/// derived_from edges)`. Shared by the plain and JSON output paths so
/// `sara memories` matches the vocabulary `recall` already established
/// (PR #79) and `sara dream` mirrors below.
pub(crate) fn canonical_labels(
    conn: &Connection,
    item: &crate::infrastructure::model::Item,
) -> (Vec<String>, Vec<String>) {
    let uuid_str = item.uuid.to_string();
    let derived: Vec<String> = db::get_memory_links_to(conn, &uuid_str)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .filter_map(|l| db::get_item_by_uuid(conn, &l.from_uuid).ok())
        .map(|i| format!("m{}", i.display_id.unwrap_or(0)))
        .collect();
    let derived_from: Vec<String> = db::get_memory_links_from(conn, &uuid_str)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .filter_map(|l| db::get_item_by_uuid(conn, &l.to_uuid).ok())
        .map(|i| format!("m{}", i.display_id.unwrap_or(0)))
        .collect();
    (derived, derived_from)
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
