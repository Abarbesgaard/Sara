use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use crate::commands::shared::{json_strs, print_json, short_handle};
use crate::infrastructure::db;
use crate::infrastructure::memory::embedding;

mod types;
use types::ConflictCandidate;

pub const DEFAULT_CONFLICT_THRESHOLD: f32 = 0.75;

pub fn diagnose_value(
    conn: &Connection,
    threshold: f32,
    project: Option<&str>,
    limit: Option<usize>,
) -> Result<Value> {
    let memories = db::list_memories(conn)?;

    let all_links = db::all_memory_links(conn)?;
    let linked_pairs: HashSet<(String, String)> = all_links
        .iter()
        .flat_map(|l| {
            [
                (l.from_uuid.clone(), l.to_uuid.clone()),
                (l.to_uuid.clone(), l.from_uuid.clone()),
            ]
        })
        .collect();

    let vectors: HashMap<String, Vec<f32>> = db::active_embeddings(conn)?.into_iter().collect();

    struct MemInfo {
        uuid: String,
        label: String,
        body: String,
        files: Vec<String>,
        tags: Vec<String>,
    }

    let mut infos: Vec<MemInfo> = Vec::new();
    for m in &memories {
        if let Some(scope) = project {
            let projects = db::get_item_projects(conn, &m.uuid).unwrap_or_default();
            if !projects.iter().any(|p| p.eq_ignore_ascii_case(scope)) {
                continue;
            }
        }
        let uuid = m.uuid.to_string();
        let files = db::get_item_files(conn, &m.uuid).unwrap_or_default();
        let label = short_handle(m);
        let tags: Vec<String> = m.tags.iter().map(|t| t.to_lowercase()).collect();
        infos.push(MemInfo {
            uuid,
            label,
            body: m.body.clone(),
            files,
            tags,
        });
    }

    let mut by_file: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, info) in infos.iter().enumerate() {
        for f in &info.files {
            by_file.entry(f.clone()).or_default().push(i);
        }
    }

    let score = |a: &MemInfo, b: &MemInfo| -> Option<f32> {
        let va = vectors.get(&a.uuid)?;
        let vb = vectors.get(&b.uuid)?;
        Some(embedding::cosine(va, vb))
    };

    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let mut candidates: Vec<ConflictCandidate> = Vec::new();

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
                let cosine = score(a, b);
                if cosine.is_some_and(|c| c < threshold) {
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
                    label_a: a.label.clone(),
                    label_b: b.label.clone(),
                    snippet_a: a.body.chars().take(80).collect(),
                    snippet_b: b.body.chars().take(80).collect(),
                    shared_files,
                    shared_tags: vec![],
                    cosine,
                });
                seen.insert(key);
            }
        }
    }

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
            let cosine = score(a, b);
            if cosine.is_some_and(|c| c < threshold) {
                seen.insert(key);
                continue;
            }
            candidates.push(ConflictCandidate {
                label_a: a.label.clone(),
                label_b: b.label.clone(),
                snippet_a: a.body.chars().take(80).collect(),
                snippet_b: b.body.chars().take(80).collect(),
                shared_files: vec![],
                shared_tags: a.tags.clone(),
                cosine,
            });
            seen.insert(key);
        }
    }

    candidates.sort_by(|a, b| {
        let ka = a.cosine.unwrap_or(f32::NEG_INFINITY);
        let kb = b.cosine.unwrap_or(f32::NEG_INFINITY);
        kb.partial_cmp(&ka)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.label_a.cmp(&b.label_a))
            .then_with(|| a.label_b.cmp(&b.label_b))
    });

    let total = candidates.len();
    if let Some(n) = limit {
        candidates.truncate(n);
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
                "cosine":       c.cosine,
            })
        })
        .collect();

    Ok(json!({
        "conflicts": items,
        "count": items.len(),
        "total": total,
        "threshold": threshold,
        "project": project,
    }))
}

pub fn run(
    conn: &Connection,
    json_output: bool,
    threshold: f32,
    project: Option<&str>,
    limit: Option<usize>,
) -> Result<()> {
    let v = diagnose_value(conn, threshold, project, limit)?;
    let count = v["count"].as_u64().unwrap_or(0);
    let total = v["total"].as_u64().unwrap_or(count);

    if json_output {
        print_json(&v)?;
        return Ok(());
    }

    let scope = project
        .map(|p| format!(" in project '{p}'"))
        .unwrap_or_default();

    if count == 0 {
        println!("No memories look like duplicates or contradictions{scope}.");
        println!(
            "Every active memory that shares a file or tag set is either already linked \
             or too different in meaning to worry about (needs cosine >= {threshold:.2})."
        );
        return Ok(());
    }

    println!(
        "Found {total} memory pair(s){scope} that may be saying the same thing — or contradicting each other."
    );
    println!(
        "Each pair is worded alike (similarity >= {threshold:.2}) and shares a file or an identical tag set, \
         yet nothing records that you've reconciled them."
    );
    if count < total {
        println!("Showing the {count} most similar; pass --limit to see the rest.");
    }
    println!();
    println!("Legend:  <memory-a> ↔ <memory-b>  [similarity 0-1]  [what they share]");
    println!("         followed by the opening line of each memory.");
    println!();
    if let Some(conflicts) = v["conflicts"].as_array() {
        for c in conflicts {
            let la = c["label_a"].as_str().unwrap_or("?");
            let lb = c["label_b"].as_str().unwrap_or("?");
            let sa = c["snippet_a"].as_str().unwrap_or("");
            let sb = c["snippet_b"].as_str().unwrap_or("");
            let score = c["cosine"]
                .as_f64()
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".to_string());

            let shared_files: Vec<&str> = json_strs(&c["shared_files"]);
            let shared_tags: Vec<&str> = json_strs(&c["shared_tags"]);

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
                println!("  {la} ↔ {lb}  [{score}] [file: {}]", names.join(", "));
            } else {
                println!(
                    "  {la} ↔ {lb}  [{score}] [tags: {}]",
                    shared_tags.join(", ")
                );
            }
            println!("    {la}: {sa}");
            println!("    {lb}: {sb}");
            println!();
        }
    }

    println!("How to reconcile each pair:");
    println!(
        "  • Same fact, one is better        → sara learn --supersedes <old-label>   (archives the weaker one)"
    );
    println!(
        "  • One is a case of a shared pattern → sara relearn <canonical-label>       (enrich it, don't duplicate)"
    );
    println!(
        "  • Both are correct but distinct   → sara link-memory <a> similar_to <b>    (marks reviewed, hides the pair)"
    );
    println!("Do nothing and the pair keeps showing up here until you act on it.");
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/diagnose_memories/mod.rs"]
mod tests;
