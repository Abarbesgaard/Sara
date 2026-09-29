use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use crate::infrastructure::{db, memory_graph::MemoryGraph};

mod types;
use types::{Cluster, UnionFind};

pub const DEFAULT_MIN_WEIGHT: f64 = 0.5;

pub const DEFAULT_MAX_CLUSTER: usize = 8;

pub fn reflect_value(conn: &Connection, min_weight: f64, max_cluster: usize) -> Result<Value> {
    let graph = MemoryGraph::build(conn)?;
    if graph.is_empty() {
        return Ok(json!({ "clusters": [], "count": 0 }));
    }

    let index: HashMap<String, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.uuid.to_string(), i))
        .collect();

    let projects_by_uuid = projects_by_uuid(conn)?;

    let deliberate = deliberate_links(conn)?;

    let mut uf = UnionFind::new(graph.nodes.len());
    let mut eligible: Vec<(usize, usize, f64)> = Vec::new();
    for (a, b, w) in graph.edges() {
        if w < min_weight {
            continue;
        }
        let (ua, ub) = (a.to_string(), b.to_string());
        if !shares_project(&ua, &ub, &projects_by_uuid) && !deliberate.contains(&pair_key(&ua, &ub))
        {
            continue;
        }
        if let (Some(&ia), Some(&ib)) = (index.get(&ua), index.get(&ub)) {
            uf.union(ia, ib);
            eligible.push((ia, ib, w));
        }
    }

    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..graph.nodes.len() {
        let root = uf.find(i);
        components.entry(root).or_default().push(i);
    }

    let derived_parents: HashMap<String, HashSet<String>> = graph
        .nodes
        .iter()
        .map(|n| {
            let parents: HashSet<String> = db::get_memory_links_from(conn, &n.uuid.to_string())
                .unwrap_or_default()
                .into_iter()
                .filter(|l| l.relation == "derived_from")
                .map(|l| l.to_uuid)
                .collect();
            (n.uuid.to_string(), parents)
        })
        .collect();

    let mut tags_by_uuid: HashMap<String, Vec<String>> = HashMap::new();
    for m in db::list_memories(conn)? {
        let tags: Vec<String> = m.tags.iter().map(|t| t.to_lowercase()).collect();
        tags_by_uuid.insert(m.uuid.to_string(), tags);
    }

    let uuids_of = |members: &[usize]| -> Vec<String> {
        members
            .iter()
            .map(|&i| graph.nodes[i].uuid.to_string())
            .collect()
    };

    let mut pieces: Vec<Vec<usize>> = Vec::new();
    for members in components.into_values() {
        if members.len() < 2 {
            continue;
        }
        if max_cluster > 0
            && members.len() > max_cluster
            && !is_already_consolidated(&uuids_of(&members), &derived_parents)
        {
            pieces.extend(split_component(members, &eligible, max_cluster));
        } else {
            pieces.push(members);
        }
    }

    let mut clusters: Vec<Cluster> = Vec::new();
    for member_idx in &pieces {
        if member_idx.len() < 2 {
            continue;
        }
        let uuids = uuids_of(member_idx);

        if is_already_consolidated(&uuids, &derived_parents) {
            continue;
        }

        let mut sorted: Vec<usize> = member_idx.clone();
        sorted.sort_by(|&a, &b| {
            graph.nodes[b]
                .strength
                .partial_cmp(&graph.nodes[a].strength)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(graph.nodes[a].label.cmp(&graph.nodes[b].label))
        });
        let suggested_canonical = graph.nodes[sorted[0]].label.clone();
        let members: Vec<String> = sorted
            .iter()
            .map(|&i| graph.nodes[i].label.clone())
            .collect();

        let shared_tags = shared_tags(&uuids, &tags_by_uuid);

        let strength_sum: f64 = member_idx.iter().map(|&i| graph.nodes[i].strength).sum();
        let score = member_idx.len() as f64 * 100.0 + strength_sum;

        clusters.push(Cluster {
            members,
            suggested_canonical,
            shared_tags,
            score,
        });
    }

    clusters.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.suggested_canonical.cmp(&b.suggested_canonical))
    });

    let items: Vec<Value> = clusters
        .iter()
        .map(|c| {
            let proposed_links: Vec<Value> = c
                .members
                .iter()
                .filter(|m| **m != c.suggested_canonical)
                .map(|m| {
                    json!({
                        "from": m,
                        "relation": "derived_from",
                        "to": c.suggested_canonical,
                    })
                })
                .collect();
            json!({
                "members": c.members,
                "suggested_canonical": c.suggested_canonical,
                "shared_tags": c.shared_tags,
                "proposed_links": proposed_links,
                "score": c.score,
            })
        })
        .collect();

    Ok(json!({
        "clusters": items,
        "count": items.len(),
    }))
}

fn split_component(
    members: Vec<usize>,
    edges: &[(usize, usize, f64)],
    max: usize,
) -> Vec<Vec<usize>> {
    if members.len() <= max {
        return vec![members];
    }
    let local: HashMap<usize, usize> = members.iter().enumerate().map(|(k, &i)| (i, k)).collect();
    let inside: Vec<(usize, usize, f64)> = edges
        .iter()
        .copied()
        .filter(|(a, b, _)| local.contains_key(a) && local.contains_key(b))
        .collect();
    let Some(weakest) = inside.iter().map(|e| e.2).min_by(f64::total_cmp) else {
        return members.into_iter().map(|m| vec![m]).collect();
    };
    let kept: Vec<(usize, usize, f64)> = inside.into_iter().filter(|e| e.2 > weakest).collect();

    let mut uf = UnionFind::new(members.len());
    for (a, b, _) in &kept {
        uf.union(local[a], local[b]);
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (k, &m) in members.iter().enumerate() {
        groups.entry(uf.find(k)).or_default().push(m);
    }
    let mut out = Vec::new();
    for group in groups.into_values() {
        out.extend(split_component(group, &kept, max));
    }
    out
}

fn is_already_consolidated(
    uuids: &[String],
    derived_parents: &HashMap<String, HashSet<String>>,
) -> bool {
    let mut candidates: HashSet<String> = uuids.iter().cloned().collect();
    for u in uuids {
        if let Some(parents) = derived_parents.get(u) {
            candidates.extend(parents.iter().cloned());
        }
    }
    candidates.iter().any(|p| {
        uuids.iter().all(|u| {
            u == p
                || derived_parents
                    .get(u)
                    .is_some_and(|parents| parents.contains(p))
        })
    })
}

fn projects_by_uuid(conn: &Connection) -> Result<HashMap<String, HashSet<String>>> {
    let mut map: HashMap<String, HashSet<String>> = HashMap::new();
    for (uuid, project) in db::all_item_projects(conn)? {
        map.entry(uuid).or_default().insert(project);
    }
    Ok(map)
}

fn pair_key(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

fn deliberate_links(conn: &Connection) -> Result<HashSet<(String, String)>> {
    let mut set = HashSet::new();
    for link in db::all_memory_links(conn).unwrap_or_default() {
        if link.relation == "co_activated" {
            continue;
        }
        set.insert(pair_key(&link.from_uuid, &link.to_uuid));
    }
    Ok(set)
}

fn shares_project(a: &str, b: &str, projects_by_uuid: &HashMap<String, HashSet<String>>) -> bool {
    match (projects_by_uuid.get(a), projects_by_uuid.get(b)) {
        (Some(pa), Some(pb)) => pa.intersection(pb).next().is_some(),
        (None, None) => true,
        _ => false,
    }
}

fn shared_tags(uuids: &[String], tags_by_uuid: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut iter = uuids.iter();
    let first = match iter.next().and_then(|u| tags_by_uuid.get(u)) {
        Some(t) => t.clone(),
        None => return vec![],
    };
    let mut shared: Vec<String> = first;
    for u in iter {
        let set: HashSet<&String> = tags_by_uuid
            .get(u)
            .map(|t| t.iter().collect())
            .unwrap_or_default();
        shared.retain(|t| set.contains(t));
    }
    shared.sort();
    shared.dedup();
    shared
}

pub fn apply_value(conn: &Connection, min_weight: f64, max_cluster: usize) -> Result<Value> {
    let proposal = reflect_value(conn, min_weight, max_cluster)?;
    let clusters = proposal["clusters"].as_array().cloned().unwrap_or_default();

    let mut applied: Vec<Value> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();

    for c in &clusters {
        let canonical = c["suggested_canonical"].as_str().unwrap_or_default();
        for link in c["proposed_links"].as_array().into_iter().flatten() {
            let from = link["from"].as_str().unwrap_or_default();
            let (from_uuid, to_uuid) = match (
                db::get_item_by_handle(conn, from),
                db::get_item_by_handle(conn, canonical),
            ) {
                (Ok(f), Ok(t)) => (f.uuid.to_string(), t.uuid.to_string()),
                _ => {
                    skipped.push(json!({
                        "from": from,
                        "to": canonical,
                        "relation": "derived_from",
                        "reason": "could not resolve memory label",
                    }));
                    continue;
                }
            };
            match db::insert_memory_link(conn, &from_uuid, &to_uuid, "derived_from", 1.0) {
                Ok(()) => applied.push(json!({
                    "from": from,
                    "relation": "derived_from",
                    "to": canonical,
                })),
                Err(e) => skipped.push(json!({
                    "from": from,
                    "to": canonical,
                    "relation": "derived_from",
                    "reason": e.to_string(),
                })),
            }
        }
    }

    Ok(json!({
        "applied": applied.len(),
        "links": applied,
        "skipped": skipped,
        "clusters": clusters.len(),
    }))
}

pub fn run(
    conn: &Connection,
    min_weight: f64,
    max_cluster: usize,
    json_output: bool,
    apply: bool,
) -> Result<()> {
    if apply {
        let v = apply_value(conn, min_weight, max_cluster)?;
        if json_output {
            println!("{}", serde_json::to_string_pretty(&v)?);
            return Ok(());
        }
        let applied = v["applied"].as_u64().unwrap_or(0);
        let skipped = v["skipped"].as_array().map(|a| a.len()).unwrap_or(0);
        if applied == 0 && skipped == 0 {
            println!(
                "Nothing to consolidate — no un-linked related clusters above the weight threshold."
            );
            return Ok(());
        }
        println!(
            "Consolidated {applied} link(s) across {} cluster(s):",
            v["clusters"].as_u64().unwrap_or(0)
        );
        for link in v["links"].as_array().into_iter().flatten() {
            println!(
                "  {} derived_from {}",
                link["from"].as_str().unwrap_or("?"),
                link["to"].as_str().unwrap_or("?"),
            );
        }
        if skipped > 0 {
            println!("\nSkipped {skipped} link(s):");
            for s in v["skipped"].as_array().into_iter().flatten() {
                println!(
                    "  {} -> {} : {}",
                    s["from"].as_str().unwrap_or("?"),
                    s["to"].as_str().unwrap_or("?"),
                    s["reason"].as_str().unwrap_or(""),
                );
            }
        }
        return Ok(());
    }

    let v = reflect_value(conn, min_weight, max_cluster)?;
    let count = v["count"].as_u64().unwrap_or(0);

    if json_output {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }

    if count == 0 {
        println!(
            "Nothing to consolidate — no un-linked related clusters above the weight threshold."
        );
        return Ok(());
    }

    println!("{count} consolidation candidate(s) — related memories with no shared canonical:");
    println!();
    if let Some(clusters) = v["clusters"].as_array() {
        for c in clusters {
            let canonical = c["suggested_canonical"].as_str().unwrap_or("?");
            let members: Vec<&str> = c["members"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();
            let tags: Vec<&str> = c["shared_tags"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();

            let tag_str = if tags.is_empty() {
                String::new()
            } else {
                format!("  [shared tags: {}]", tags.join(", "))
            };
            println!(
                "  {} members, canonical -> {canonical}{tag_str}",
                members.len()
            );
            println!("    cluster: {}", members.join(", "));
            for link in c["proposed_links"].as_array().into_iter().flatten() {
                let from = link["from"].as_str().unwrap_or("?");
                println!("    sara link-memory {from} derived_from {canonical}");
            }
            println!();
        }
    }
    println!("Review each cluster, then run the printed `sara link-memory` lines to consolidate.");
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/reflect/mod.rs"]
mod tests;
