use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use crate::infrastructure::{db, memory_graph::MemoryGraph};

/// Default minimum synapse weight for two memories to be considered clustered.
/// Above a single weak shared tag (`W_SHARED_TAG` = 0.3) so broad tag crowding
/// does not explode into noise; a shared file (0.6), shared task (0.8), an
/// explicit link, or several summed anchors clears it.
pub const DEFAULT_MIN_WEIGHT: f64 = 0.5;

/// A proposed consolidation: a cluster of related, not-yet-consolidated memories
/// and the canonical+derived_from restructuring that would tidy them.
#[derive(Debug)]
struct Cluster {
    /// Member labels (e.g. `m209`), suggested canonical first.
    members: Vec<String>,
    /// Label of the member proposed to become the canonical (strongest member).
    suggested_canonical: String,
    /// Tags shared by every member (may be empty).
    shared_tags: Vec<String>,
    /// Ranking score: size dominates, strength breaks ties.
    score: f64,
}

/// Union-find over memory indices for transitive clustering.
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind { parent: (0..n).collect() }
    }
    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.parent[r] != r {
            r = self.parent[r];
        }
        // Path compression.
        let mut cur = x;
        while self.parent[cur] != r {
            let next = self.parent[cur];
            self.parent[cur] = r;
            cur = next;
        }
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}

/// Core shared by CLI and MCP. Clusters related memories via the MemoryGraph and
/// proposes canonical+derived_from consolidations for clusters not yet tidied.
///
/// A cluster is a connected component (over synapses of weight >= `min_weight`)
/// of two or more memories. A component is **excluded** when it is already
/// consolidated: there exists a memory `P` such that every member either *is* `P`
/// or has an outgoing `derived_from` edge to `P` (i.e. a canonical and its
/// children, or siblings under a shared parent).
pub fn reflect_value(conn: &Connection, min_weight: f64) -> Result<Value> {
    let graph = MemoryGraph::build(conn)?;
    if graph.is_empty() {
        return Ok(json!({ "clusters": [], "count": 0 }));
    }

    // uuid -> node index.
    let index: HashMap<String, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.uuid.to_string(), i))
        .collect();

    // Cluster via union-find over strong-enough edges.
    let mut uf = UnionFind::new(graph.nodes.len());
    for (a, b, w) in graph.edges() {
        if w < min_weight {
            continue;
        }
        if let (Some(&ia), Some(&ib)) = (index.get(&a.to_string()), index.get(&b.to_string())) {
            uf.union(ia, ib);
        }
    }

    // Group node indices by component root.
    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..graph.nodes.len() {
        let root = uf.find(i);
        components.entry(root).or_default().push(i);
    }

    // Per-memory outgoing `derived_from` parents (uuids), for the exclusion rule.
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

    // Per-memory normalised tag set, for shared-tag intersection.
    let mut tags_by_uuid: HashMap<String, Vec<String>> = HashMap::new();
    for m in db::list_memories(conn)? {
        let tags: Vec<String> = m.tags.iter().map(|t| t.to_lowercase()).collect();
        tags_by_uuid.insert(m.uuid.to_string(), tags);
    }

    let mut clusters: Vec<Cluster> = Vec::new();
    for member_idx in components.values() {
        if member_idx.len() < 2 {
            continue;
        }
        let uuids: Vec<String> =
            member_idx.iter().map(|&i| graph.nodes[i].uuid.to_string()).collect();

        if is_already_consolidated(&uuids, &derived_parents) {
            continue;
        }

        // Strongest member becomes the suggested canonical.
        let mut sorted: Vec<usize> = member_idx.clone();
        sorted.sort_by(|&a, &b| {
            graph.nodes[b]
                .strength
                .partial_cmp(&graph.nodes[a].strength)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(graph.nodes[a].label.cmp(&graph.nodes[b].label))
        });
        let suggested_canonical = graph.nodes[sorted[0]].label.clone();
        let members: Vec<String> = sorted.iter().map(|&i| graph.nodes[i].label.clone()).collect();

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

    // Rank: biggest, strongest clusters first; stable by canonical label.
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

/// A component is already consolidated when some memory `P` is a common parent:
/// every member either *is* `P` or has a `derived_from` edge to `P`.
fn is_already_consolidated(
    uuids: &[String],
    derived_parents: &HashMap<String, HashSet<String>>,
) -> bool {
    // Candidate parents: any member, plus any parent any member points at.
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

/// Tags present on every member of the cluster.
fn shared_tags(uuids: &[String], tags_by_uuid: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut iter = uuids.iter();
    let first = match iter.next().and_then(|u| tags_by_uuid.get(u)) {
        Some(t) => t.clone(),
        None => return vec![],
    };
    let mut shared: Vec<String> = first;
    for u in iter {
        let set: HashSet<&String> =
            tags_by_uuid.get(u).map(|t| t.iter().collect()).unwrap_or_default();
        shared.retain(|t| set.contains(t));
    }
    shared.sort();
    shared.dedup();
    shared
}

/// `sara reflect` — propose canonical+derived consolidations for clusters of
/// related, not-yet-tidied memories.
pub fn run(conn: &Connection, min_weight: f64, json_output: bool) -> Result<()> {
    let v = reflect_value(conn, min_weight)?;
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
            println!("  {} members, canonical -> {canonical}{tag_str}", members.len());
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
mod tests {
    use crate::infrastructure::{db, model::Item};
    use uuid::Uuid;

    /// Insert a tagged memory (no anchors — edges are added explicitly per test).
    fn insert_memory(conn: &rusqlite::Connection, body: &str, tag: &str) -> Uuid {
        let mut item = Item::new_memory(body.to_string(), body.to_string(), None);
        item.tags = vec![tag.to_string()];
        item.path = Some(String::new());
        db::insert_item(conn, &mut item).unwrap();
        item.uuid
    }

    fn link(conn: &rusqlite::Connection, from: &Uuid, to: &Uuid, relation: &str) {
        db::insert_memory_link(conn, &from.to_string(), &to.to_string(), relation, 1.0).unwrap();
    }

    #[test]
    fn fresh_related_cluster_is_proposed_with_a_canonical() {
        let conn = db::open_in_memory_for_test();
        // Three memories flagged related (similar_to, 0.7 each) but with no
        // canonical yet — the exact shape a consolidation should propose.
        let a = insert_memory(&conn, "dependabot bump broke restore in repo a", "dependabot");
        let b = insert_memory(&conn, "dependabot bump broke restore in repo b", "dependabot");
        let c = insert_memory(&conn, "dependabot bump broke restore in repo c", "dependabot");
        link(&conn, &a, &b, "similar_to");
        link(&conn, &b, &c, "similar_to");

        let v = super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT).unwrap();
        assert_eq!(v["count"].as_u64().unwrap(), 1, "one cluster expected");
        let cluster = &v["clusters"][0];
        assert_eq!(cluster["members"].as_array().unwrap().len(), 3);
        assert!(!cluster["suggested_canonical"].as_str().unwrap().is_empty());
        // Two derived_from links proposed (canonical + 2 children).
        assert_eq!(cluster["proposed_links"].as_array().unwrap().len(), 2);
        // The shared tag is surfaced.
        assert!(cluster["shared_tags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "dependabot"));
    }

    #[test]
    fn already_consolidated_cluster_is_excluded() {
        let conn = db::open_in_memory_for_test();
        let canonical = insert_memory(&conn, "canonical pattern", "dependabot");
        let child_a = insert_memory(&conn, "applied in repo a", "dependabot");
        let child_b = insert_memory(&conn, "applied in repo b", "dependabot");

        // Both children point at the canonical — the cluster is already tidy.
        // (derived_from carries weight 0.8, so they still form one component.)
        link(&conn, &child_a, &canonical, "derived_from");
        link(&conn, &child_b, &canonical, "derived_from");

        let v = super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT).unwrap();
        assert_eq!(
            v["count"].as_u64().unwrap(),
            0,
            "a canonical + its derived children must not be re-proposed"
        );
    }

    #[test]
    fn unrelated_memories_are_not_clustered() {
        let conn = db::open_in_memory_for_test();
        insert_memory(&conn, "memory about auth", "auth");
        insert_memory(&conn, "memory about billing", "billing");

        let v = super::reflect_value(&conn, super::DEFAULT_MIN_WEIGHT).unwrap();
        assert_eq!(v["count"].as_u64().unwrap(), 0, "no shared anchors -> no cluster");
    }
}
