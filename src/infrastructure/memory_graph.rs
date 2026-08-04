//! The memory graph — Sara's nervous system.
//!
//! Memories are neurons; their associations are synapses. This module lifts the
//! weighted associative graph that `sara dream` already assembles for its
//! constellation view out of the TUI and makes it a first-class retrieval
//! structure, so recall can *spread activation* across it instead of returning
//! only the memories a query matched directly.
//!
//! Two synapse classes, exactly as `dream`'s force layout already models them:
//!   - **explicit** edges — user/agent-authored `memory_links`
//!     (`supersedes` / `similar_to` / `derived_from` / `used_in`) plus the
//!     machine-learned `co_activated` relation.
//!   - **implicit** edges — shared anchors: two memories tied to the same tag,
//!     file, or task are associated, weighted by how specific that anchor is
//!     (a shared task binds tighter than a shared tag).
//!
//! Plasticity is Hebbian: memories that surface together in one recall "fire
//! together", and [`consolidate`] turns those co-firings (read from the
//! `memory_recalled` event log) into reinforced `co_activated` edge weight, so
//! the graph learns its own wiring from use. Disused nodes and their boosts
//! already decay elsewhere (see `db::item_strength`), so the network stays lean.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use std::collections::HashMap;
use uuid::Uuid;

use crate::infrastructure::db;

// ── synapse weights ─────────────────────────────────────────────────────────
// Implicit (shared-anchor) edge weight per shared anchor of each kind. A shared
// task is the strongest signal (both memories came out of the same work), a
// shared tag the weakest. These mirror the stiffness ordering `dream`'s force
// layout already uses (firm bonds vs soft tag springs), promoted to retrieval.
const W_SHARED_TASK: f64 = 0.8;
const W_SHARED_FILE: f64 = 0.6;
const W_SHARED_TAG: f64 = 0.3;

/// Explicit `memory_links` relation → base synapse weight. Multiplied by the
/// edge's stored `weight` (1.0 by default; the learned magnitude for
/// `co_activated`).
fn relation_weight(relation: &str, stored: f64) -> f64 {
    let base = match relation {
        "derived_from" => 0.8,
        "similar_to" => 0.7,
        "used_in" => 0.6,
        "supersedes" => 0.5,
        // Learned Hebbian edge: the stored weight already *is* the magnitude,
        // scaled down so a single co-firing is a whisper, not a shout.
        "co_activated" => return (0.25 * stored).min(MAX_EDGE),
        _ => 0.4,
    };
    (base * stored).min(MAX_EDGE)
}

/// No single edge may exceed this, so a densely-anchored pair can't dominate.
const MAX_EDGE: f64 = 1.0;

// ── graph ───────────────────────────────────────────────────────────────────

/// One neuron: a memory, with its display label and current activation ceiling
/// (`item_strength`). A Strong memory radiates more when it fires.
#[derive(Debug, Clone)]
pub struct Node {
    pub uuid: Uuid,
    pub label: String,
    pub strength: f64,
}

/// The whole nervous system: neurons plus a symmetric weighted adjacency.
#[derive(Debug, Default)]
pub struct MemoryGraph {
    pub nodes: Vec<Node>,
    index: HashMap<Uuid, usize>,
    /// Undirected adjacency: `adj[i]` = `(neighbour_index, weight)`.
    adj: Vec<Vec<(usize, f64)>>,
}

impl MemoryGraph {
    /// Assemble the graph from the store: every active/provisional memory
    /// becomes a node; explicit `memory_links` and shared-anchor overlaps
    /// become weighted undirected edges. Parallel edges between the same pair
    /// (e.g. an explicit link *and* a shared file) sum, capped at [`MAX_EDGE`].
    pub fn build(conn: &Connection) -> Result<MemoryGraph> {
        let memories = db::list_memories(conn)?;

        let mut nodes = Vec::with_capacity(memories.len());
        let mut index = HashMap::with_capacity(memories.len());
        // Per-node anchor sets, preloaded once so pairing is in-memory (no
        // per-pair queries) — the same O(n²) shape `dream` already runs.
        let mut tag_sets: Vec<Vec<String>> = Vec::with_capacity(memories.len());
        let mut file_sets: Vec<Vec<String>> = Vec::with_capacity(memories.len());
        let mut task_sets: Vec<Vec<Uuid>> = Vec::with_capacity(memories.len());

        for (i, m) in memories.iter().enumerate() {
            index.insert(m.uuid, i);
            nodes.push(Node {
                uuid: m.uuid,
                label: format!("m{}", m.display_id.unwrap_or(0)),
                strength: db::item_strength(conn, m),
            });
            tag_sets.push(m.tags.clone());
            file_sets.push(db::get_item_files(conn, &m.uuid).unwrap_or_default());
            task_sets.push(
                db::get_item_task_links(conn, &m.uuid)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(t, _)| t.uuid)
                    .collect(),
            );
        }

        // Accumulate every edge into a single (min,max)->weight map so parallel
        // synapses merge deterministically regardless of discovery order.
        let mut edges: HashMap<(usize, usize), f64> = HashMap::new();
        let add = |a: usize, b: usize, w: f64, edges: &mut HashMap<(usize, usize), f64>| {
            if a == b || w <= 0.0 {
                return;
            }
            let key = if a < b { (a, b) } else { (b, a) };
            let e = edges.entry(key).or_insert(0.0);
            *e = (*e + w).min(MAX_EDGE);
        };

        // Explicit edges.
        for link in db::all_memory_links(conn).unwrap_or_default() {
            let (Ok(fu), Ok(tu)) =
                (Uuid::parse_str(&link.from_uuid), Uuid::parse_str(&link.to_uuid))
            else {
                continue;
            };
            if let (Some(&a), Some(&b)) = (index.get(&fu), index.get(&tu)) {
                add(a, b, relation_weight(&link.relation, link.weight), &mut edges);
            }
        }

        // Implicit (shared-anchor) edges.
        let n = nodes.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let mut w = 0.0;
                w += W_SHARED_TAG * count_shared(&tag_sets[i], &tag_sets[j]) as f64;
                w += W_SHARED_FILE * count_shared(&file_sets[i], &file_sets[j]) as f64;
                w += W_SHARED_TASK * count_shared(&task_sets[i], &task_sets[j]) as f64;
                if w > 0.0 {
                    add(i, j, w, &mut edges);
                }
            }
        }

        let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        // Deterministic adjacency order: sort keys before inserting.
        let mut keys: Vec<(usize, usize)> = edges.keys().copied().collect();
        keys.sort_unstable();
        for (a, b) in keys {
            let w = edges[&(a, b)];
            adj[a].push((b, w));
            adj[b].push((a, w));
        }

        Ok(MemoryGraph { nodes, index, adj })
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn edge_count(&self) -> usize {
        self.adj.iter().map(|v| v.len()).sum::<usize>() / 2
    }

    /// Weight of the direct edge between two memories, if any (test/introspection).
    pub fn edge_weight(&self, a: &Uuid, b: &Uuid) -> Option<f64> {
        let (&ia, &ib) = (self.index.get(a)?, self.index.get(b)?);
        self.adj[ia].iter().find(|(j, _)| *j == ib).map(|(_, w)| *w)
    }

    /// Spread activation outward from `seeds` for `hops`, attenuating by
    /// `decay` (0..1) each hop and dropping contributions below `threshold`.
    /// Seeds start charged to their own `strength`; every reached memory
    /// accumulates the activation flowing into it. Returns all activated
    /// memories (seeds included) ranked by total activation, then by node order
    /// for stable, reproducible output.
    pub fn spread_activation(
        &self,
        seeds: &[Uuid],
        hops: usize,
        decay: f64,
        threshold: f64,
    ) -> Vec<(Uuid, f64)> {
        let n = self.nodes.len();
        let mut total = vec![0.0f64; n];
        let mut layer = vec![0.0f64; n];

        for s in seeds {
            if let Some(&i) = self.index.get(s) {
                let a = self.nodes[i].strength.max(0.0);
                layer[i] += a;
                total[i] += a;
            }
        }

        for _ in 0..hops {
            let mut next = vec![0.0f64; n];
            for (i, &src) in layer.iter().enumerate() {
                if src <= threshold {
                    continue;
                }
                for &(j, w) in &self.adj[i] {
                    let delta = src * w * decay;
                    if delta > threshold {
                        next[j] += delta;
                    }
                }
            }
            for (t, nx) in total.iter_mut().zip(next.iter()) {
                *t += *nx;
            }
            layer = next;
        }

        let mut out: Vec<(usize, Uuid, f64)> = (0..n)
            .filter(|&i| total[i] > threshold)
            .map(|i| (i, self.nodes[i].uuid, total[i]))
            .collect();
        out.sort_by(|a, b| {
            b.2.partial_cmp(&a.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        out.into_iter().map(|(_, u, a)| (u, a)).collect()
    }
}

/// Count elements shared between two small unordered sets.
fn count_shared<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    a.iter().filter(|x| b.contains(x)).count()
}

// ── Hebbian consolidation ────────────────────────────────────────────────────

/// Group timestamped recall events into co-firing pairs: any two *distinct*
/// memories whose `memory_recalled` events fall in the same `bucket` window
/// fired together. Returns each unordered pair with the number of buckets in
/// which they co-fired. Pure (no DB) so it is directly unit-testable.
pub fn coactivation_pairs(
    events: &[(Uuid, DateTime<Utc>)],
    bucket: Duration,
) -> Vec<(Uuid, Uuid, u32)> {
    if events.is_empty() || bucket <= Duration::zero() {
        return vec![];
    }
    let bucket_ms = bucket.num_milliseconds().max(1);
    // Assign each event to a fixed-width time bucket, then pair within buckets.
    let mut by_bucket: HashMap<i64, Vec<Uuid>> = HashMap::new();
    for (u, at) in events {
        let b = at.timestamp_millis() / bucket_ms;
        by_bucket.entry(b).or_default().push(*u);
    }

    let mut pair_counts: HashMap<(Uuid, Uuid), u32> = HashMap::new();
    for members in by_bucket.values() {
        // Distinct memories in this bucket.
        let mut uniq: Vec<Uuid> = members.clone();
        uniq.sort_unstable();
        uniq.dedup();
        for i in 0..uniq.len() {
            for j in (i + 1)..uniq.len() {
                let key = if uniq[i] < uniq[j] {
                    (uniq[i], uniq[j])
                } else {
                    (uniq[j], uniq[i])
                };
                *pair_counts.entry(key).or_insert(0) += 1;
            }
        }
    }

    let mut out: Vec<(Uuid, Uuid, u32)> =
        pair_counts.into_iter().map(|((a, b), c)| (a, b, c)).collect();
    out.sort_unstable_by(|x, y| y.2.cmp(&x.2).then(x.0.cmp(&y.0)).then(x.1.cmp(&y.1)));
    out
}

/// Hebbian consolidation pass: read the last `window_days` of recall events,
/// find co-firing pairs (within `bucket`), and reinforce a `co_activated` edge
/// for each by `delta` per co-firing. Returns the number of pairs reinforced.
/// Idempotent in spirit — weight simply accumulates, and disused edges decay
/// with their nodes elsewhere.
pub fn consolidate(
    conn: &Connection,
    window_days: i64,
    bucket: Duration,
    delta: f64,
) -> Result<usize> {
    let cutoff = Utc::now() - Duration::days(window_days.max(0));
    let events = db::memory_recall_events_since(conn, &cutoff)?;
    let pairs = coactivation_pairs(&events, bucket);
    for (a, b, count) in &pairs {
        db::reinforce_coactivation(conn, &a.to_string(), &b.to_string(), delta * *count as f64)?;
    }
    Ok(pairs.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::model::Item;

    fn seed(conn: &Connection, tags: &[&str]) -> Uuid {
        let mut item = Item::new_memory("t".into(), "body".into(), None);
        item.tags = tags.iter().map(|s| s.to_string()).collect();
        item.path = Some(String::new());
        db::insert_item(conn, &mut item).unwrap();
        item.uuid
    }

    #[test]
    fn shared_tag_creates_a_weighted_edge() {
        let conn = db::open_in_memory_for_test();
        let a = seed(&conn, &["auth"]);
        let b = seed(&conn, &["auth"]);
        let c = seed(&conn, &["billing"]);

        let g = MemoryGraph::build(&conn).unwrap();
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.edge_weight(&a, &b), Some(W_SHARED_TAG));
        assert_eq!(g.edge_weight(&a, &c), None);
    }

    #[test]
    fn explicit_link_and_shared_anchor_sum() {
        let conn = db::open_in_memory_for_test();
        let a = seed(&conn, &["auth"]);
        let b = seed(&conn, &["auth"]);
        db::insert_memory_link(&conn, &a.to_string(), &b.to_string(), "similar_to", 1.0).unwrap();

        let g = MemoryGraph::build(&conn).unwrap();
        // shared tag (0.3) + similar_to (0.7) = 1.0.
        assert!((g.edge_weight(&a, &b).unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn activation_spreads_to_two_hop_neighbour_and_decays() {
        let conn = db::open_in_memory_for_test();
        // Chain a — b — c via shared tags (a,b share "x"; b,c share "y").
        let a = seed(&conn, &["x"]);
        let b = seed(&conn, &["x", "y"]);
        let c = seed(&conn, &["y"]);

        let g = MemoryGraph::build(&conn).unwrap();
        let ranked = g.spread_activation(&[a], 2, 0.6, 1e-6);
        let act: HashMap<Uuid, f64> = ranked.into_iter().collect();

        // Seed strongest, direct neighbour next, 2-hop weakest but present.
        assert!(act[&a] > act[&b]);
        assert!(act[&b] > act[&c]);
        assert!(act[&c] > 0.0, "two-hop neighbour must be activated");
    }

    #[test]
    fn unconnected_memory_is_not_activated() {
        let conn = db::open_in_memory_for_test();
        let a = seed(&conn, &["x"]);
        let lone = seed(&conn, &["unrelated"]);
        let g = MemoryGraph::build(&conn).unwrap();
        let reached: Vec<Uuid> = g
            .spread_activation(&[a], 3, 0.6, 1e-6)
            .into_iter()
            .map(|(u, _)| u)
            .collect();
        assert!(reached.contains(&a));
        assert!(!reached.contains(&lone));
    }

    #[test]
    fn coactivation_pairs_group_within_bucket() {
        let t0 = Utc::now();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let events = vec![
            (a, t0),
            (b, t0 + Duration::milliseconds(100)), // same bucket as a
            (c, t0 + Duration::seconds(60)),        // far away — own bucket
        ];
        let pairs = coactivation_pairs(&events, Duration::seconds(2));
        assert_eq!(pairs.len(), 1);
        let (x, y, count) = pairs[0];
        assert_eq!(count, 1);
        let got = if x < y { (x, y) } else { (y, x) };
        let want = if a < b { (a, b) } else { (b, a) };
        assert_eq!(got, want);
    }

    #[test]
    fn consolidate_reinforces_a_co_activated_edge_from_recall_events() {
        let conn = db::open_in_memory_for_test();
        let a = seed(&conn, &["x"]);
        let b = seed(&conn, &["y"]); // no shared anchor — only co-firing links them

        // Two recalls in which a and b both surfaced.
        for _ in 0..2 {
            db::record_memory_recall(&conn, &a).unwrap();
            db::record_memory_recall(&conn, &b).unwrap();
        }

        let reinforced = consolidate(&conn, 30, Duration::seconds(2), 0.1).unwrap();
        assert_eq!(reinforced, 1);

        // The learned edge now exists in the graph despite no shared anchor.
        let g = MemoryGraph::build(&conn).unwrap();
        assert!(
            g.edge_weight(&a, &b).is_some(),
            "co-activation should have wired a and b together"
        );
    }
}
