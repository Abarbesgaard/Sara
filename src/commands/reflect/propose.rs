use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use super::consolidated::is_already_consolidated;
use super::eligibility::{deliberate_links, pair_key, projects_by_uuid, shares_project};
use super::split::split_component;
use super::tags::shared_tags;
use super::types::{Cluster, UnionFind};
use crate::infrastructure::db;
use crate::infrastructure::memory::graph::MemoryGraph;

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

    let items: Vec<Value> = clusters.iter().map(cluster_json).collect();

    Ok(json!({
        "clusters": items,
        "count": items.len(),
    }))
}

fn cluster_json(c: &Cluster) -> Value {
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
}
