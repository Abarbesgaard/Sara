use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use crate::infrastructure::db;

/// A pair of memories that may be in conflict (no memory_links edge between them).
#[derive(Debug)]
pub struct ConflictCandidate {
    pub label_a:     String,
    pub label_b:     String,
    pub snippet_a:   String,
    pub snippet_b:   String,
    pub shared_files: Vec<String>,
    pub shared_tags:  Vec<String>,
}

/// Core shared by CLI and MCP. Scans all active memories and returns conflict
/// candidates: pairs sharing file-path links OR full tag sets with no
/// memory_links edge between them in either direction.
pub fn diagnose_value(conn: &Connection) -> Result<Value> {
    let memories = db::list_memories(conn)?;

    // Build a set of (uuid, uuid) pairs that already have a link edge.
    let all_links = db::all_memory_links(conn)?;
    let linked_pairs: HashSet<(String, String)> = all_links
        .iter()
        .flat_map(|l| {
            // Treat edges as undirected for conflict-suppression purposes.
            [
                (l.from_uuid.clone(), l.to_uuid.clone()),
                (l.to_uuid.clone(), l.from_uuid.clone()),
            ]
        })
        .collect();

    // For each memory, collect its files and normalised tags.
    struct MemInfo {
        uuid:  String,
        label: String,
        body:  String,
        files: Vec<String>,
        tags:  Vec<String>,
    }

    let mut infos: Vec<MemInfo> = Vec::new();
    for m in &memories {
        let uuid = m.uuid.to_string();
        let files = db::get_item_files(conn, &m.uuid).unwrap_or_default();
        let label = m
            .display_id
            .map(|id| format!("m{id}"))
            .unwrap_or_else(|| uuid[..8].to_string());
        let tags: Vec<String> = m.tags.iter().map(|t| t.to_lowercase()).collect();
        infos.push(MemInfo {
            uuid,
            label,
            body: m.body.clone(),
            files,
            tags,
        });
    }

    // Index memories by file path.
    let mut by_file: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, info) in infos.iter().enumerate() {
        for f in &info.files {
            by_file.entry(f.clone()).or_default().push(i);
        }
    }

    // Collect conflict candidates — deduplicate with a seen set.
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let mut candidates: Vec<ConflictCandidate> = Vec::new();

    // 1. File-path overlap.
    for indices in by_file.values() {
        for &i in indices {
            for &j in indices {
                if i >= j {
                    continue;
                }
                let key = (i.min(j), i.max(j));
                if seen.contains(&key) {
                    continue;
                }
                let a = &infos[i];
                let b = &infos[j];
                if linked_pairs.contains(&(a.uuid.clone(), b.uuid.clone())) {
                    seen.insert(key);
                    continue;
                }
                let shared_files: Vec<String> = a
                    .files
                    .iter()
                    .filter(|f| b.files.contains(f))
                    .cloned()
                    .collect();
                candidates.push(ConflictCandidate {
                    label_a:     a.label.clone(),
                    label_b:     b.label.clone(),
                    snippet_a:   a.body.chars().take(80).collect(),
                    snippet_b:   b.body.chars().take(80).collect(),
                    shared_files,
                    shared_tags:  vec![],
                });
                seen.insert(key);
            }
        }
    }

    // 2. Full tag-set overlap (all tags identical, no link).
    for i in 0..infos.len() {
        for j in (i + 1)..infos.len() {
            let key = (i, j);
            if seen.contains(&key) {
                continue;
            }
            let a = &infos[i];
            let b = &infos[j];
            if a.tags.is_empty() || b.tags.is_empty() {
                continue;
            }
            let a_set: HashSet<&String> = a.tags.iter().collect();
            let b_set: HashSet<&String> = b.tags.iter().collect();
            if a_set != b_set {
                continue;
            }
            if linked_pairs.contains(&(a.uuid.clone(), b.uuid.clone())) {
                seen.insert(key);
                continue;
            }
            candidates.push(ConflictCandidate {
                label_a:     a.label.clone(),
                label_b:     b.label.clone(),
                snippet_a:   a.body.chars().take(80).collect(),
                snippet_b:   b.body.chars().take(80).collect(),
                shared_files: vec![],
                shared_tags:  a.tags.clone(),
            });
            seen.insert(key);
        }
    }

    let items: Vec<Value> = candidates
        .iter()
        .map(|c| {
            json!({
                "label_a":      c.label_a,
                "label_b":      c.label_b,
                "snippet_a":    c.snippet_a,
                "snippet_b":    c.snippet_b,
                "shared_files": c.shared_files,
                "shared_tags":  c.shared_tags,
            })
        })
        .collect();

    Ok(json!({
        "conflicts": items,
        "count": items.len(),
    }))
}

/// `sara diagnose-memories` (alias: `sara conflicts`)
pub fn run(conn: &Connection, json_output: bool) -> Result<()> {
    let v = diagnose_value(conn)?;
    let count = v["count"].as_u64().unwrap_or(0);

    if json_output {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }

    if count == 0 {
        println!("No unlinked conflict candidates found.");
        return Ok(());
    }

    println!("{count} potential conflict(s) — memories sharing files or tags with no link:");
    println!();
    if let Some(conflicts) = v["conflicts"].as_array() {
        for c in conflicts {
            let la = c["label_a"].as_str().unwrap_or("?");
            let lb = c["label_b"].as_str().unwrap_or("?");
            let sa = c["snippet_a"].as_str().unwrap_or("");
            let sb = c["snippet_b"].as_str().unwrap_or("");

            let shared_files: Vec<&str> = c["shared_files"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();
            let shared_tags: Vec<&str> = c["shared_tags"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();

            if !shared_files.is_empty() {
                let names: Vec<&str> = shared_files
                    .iter()
                    .map(|p| {
                        std::path::Path::new(p)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(p)
                    })
                    .collect();
                println!("  {la} ↔ {lb}  [file: {}]", names.join(", "));
            } else {
                println!("  {la} ↔ {lb}  [tags: {}]", shared_tags.join(", "));
            }
            println!("    {la}: {sa}");
            println!("    {lb}: {sb}");
            println!();
        }
    }

    println!("Resolve with: sara relearn <label> / sara learn --supersedes <label> / sara link-memory <a> similar_to <b>");
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::infrastructure::{db, model::Item};
    use uuid::Uuid;

    fn insert_memory_with_file(conn: &rusqlite::Connection, body: &str, tag: &str, file: &str) -> Uuid {
        let mut item = Item::new_memory(body.to_string(), body.to_string(), None);
        item.tags = vec![tag.to_string()];
        item.path = Some(String::new());
        db::insert_item(conn, &mut item).unwrap();
        db::set_item_projects(conn, &item.uuid, &["test".to_string()]).unwrap();
        db::set_item_files(conn, &item.uuid, &[file.to_string()]).unwrap();
        item.uuid
    }

    #[test]
    fn two_memories_on_same_file_appear_in_diagnose() {
        let conn = db::open_in_memory_for_test();
        let file = "/tmp/sara_diag_test.rs".to_string();
        insert_memory_with_file(&conn, "first memory", "tag-a", &file);
        insert_memory_with_file(&conn, "second memory different tags", "tag-b", &file);

        let v = super::diagnose_value(&conn).unwrap();
        assert_eq!(v["count"].as_u64().unwrap(), 1);
    }

    #[test]
    fn linked_memories_do_not_appear_in_diagnose() {
        let conn = db::open_in_memory_for_test();
        let file = "/tmp/sara_diag_linked_test.rs".to_string();
        let uuid_a = insert_memory_with_file(&conn, "memory a", "tag-c", &file);
        let uuid_b = insert_memory_with_file(&conn, "memory b", "tag-d", &file);

        // Link them — they should disappear from diagnose output.
        db::insert_memory_link(&conn, &uuid_a.to_string(), &uuid_b.to_string(), "supersedes", 1.0).unwrap();

        let v = super::diagnose_value(&conn).unwrap();
        assert_eq!(v["count"].as_u64().unwrap(), 0);
    }
}
