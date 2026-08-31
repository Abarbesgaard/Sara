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

        // All per-memory anchors and scores in a handful of bulk queries instead
        // of several per node: recall boosts, base strengths, file anchors and
        // task anchors are each fetched once for the whole store.
        let boosts = db::recall_usage_boosts(conn);
        let base_strengths = db::item_base_strengths(conn, &memories);
        let canonical_bonuses = db::canonical_derived_bonuses(conn);
        let mut all_files = db::all_item_files(conn);
        let mut all_tasks = db::all_item_task_uuids(conn);

        for (i, m) in memories.iter().enumerate() {
            index.insert(m.uuid, i);
            nodes.push(Node {
                uuid: m.uuid,
                label: format!("m{}", m.display_id.unwrap_or(0)),
                strength: base_strengths.get(&m.uuid).copied().unwrap_or(1.0)
                    + boosts.get(&m.uuid).copied().unwrap_or(0.0)
                    + canonical_bonuses.get(&m.uuid).copied().unwrap_or(0.0),
            });
            // Dedup each anchor set: a memory carrying the same tag/file/task
            // twice must count once, so a duplicated anchor can't inflate a
            // pair's shared-anchor weight (document frequencies already dedup,
            // so this keeps both sides of the IDF consistent).
            tag_sets.push(dedup(m.tags.clone()));
            file_sets.push(dedup(all_files.remove(&m.uuid).unwrap_or_default()));
            task_sets.push(dedup(all_tasks.remove(&m.uuid).unwrap_or_default()));
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
            let (Ok(fu), Ok(tu)) = (
                Uuid::parse_str(&link.from_uuid),
                Uuid::parse_str(&link.to_uuid),
            ) else {
                continue;
            };
            if let (Some(&a), Some(&b)) = (index.get(&fu), index.get(&tu)) {
                add(
                    a,
                    b,
                    relation_weight(&link.relation, link.weight),
                    &mut edges,
                );
            }
        }

        // Implicit (shared-anchor) edges, IDF-weighted: a shared anchor binds
        // in inverse proportion to how common it is. A tag on nearly every
        // memory carries almost no associative signal (idf → 0); a tag on just
        // two binds near its base weight. Without this, ubiquitous anchors
        // (e.g. a `memory` tag on a third of the store) over-connect the graph
        // until spreading activation degenerates into global centrality.
        let n = nodes.len();
        let tag_df = document_frequencies(&tag_sets);
        let file_df = document_frequencies(&file_sets);
        let task_df = document_frequencies(&task_sets);
        let idf = |df: usize| -> f64 {
            if n < 2 || df == 0 || df >= n {
                return 0.0;
            }
            (n as f64 / df as f64).ln() / (n as f64).ln()
        };
        for i in 0..n {
            for j in (i + 1)..n {
                let mut w = 0.0;
                for t in shared(&tag_sets[i], &tag_sets[j]) {
                    w += W_SHARED_TAG * idf(*tag_df.get(t).unwrap_or(&0));
                }
                for f in shared(&file_sets[i], &file_sets[j]) {
                    w += W_SHARED_FILE * idf(*file_df.get(f).unwrap_or(&0));
                }
                for t in shared(&task_sets[i], &task_sets[j]) {
                    w += W_SHARED_TASK * idf(*task_df.get(t).unwrap_or(&0));
                }
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

    /// Every undirected synapse once, as `(a_uuid, b_uuid, weight)`. For
    /// consumers that drive geometry from synapse strength — `sara dream`'s
    /// constellation uses these calibrated weights as its spring stiffness,
    /// so the same rare-anchor-binds-tighter shape recall spreads over is what
    /// the web draws.
    pub fn edges(&self) -> Vec<(Uuid, Uuid, f64)> {
        let mut out = Vec::with_capacity(self.edge_count());
        for (i, neighbours) in self.adj.iter().enumerate() {
            for &(j, w) in neighbours {
                if i < j {
                    out.push((self.nodes[i].uuid, self.nodes[j].uuid, w));
                }
            }
        }
        out
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

    /// Like [`spread_activation`], but also reconstructs *why* each memory lit
    /// up: the strongest synaptic path back to a seed. For every activated node
    /// we remember the single neighbour that delivered the largest activation
    /// contribution; following those predecessors yields the dominant path
    /// (`seed → … → node`), returned as memory labels. Seeds have a one-element
    /// path (themselves). Ranking and thresholds match `spread_activation`.
    pub fn spread_activation_explained(
        &self,
        seeds: &[Uuid],
        hops: usize,
        decay: f64,
        threshold: f64,
    ) -> Vec<Activation> {
        let n = self.nodes.len();
        let mut total = vec![0.0f64; n];
        let mut layer = vec![0.0f64; n];
        // Strongest single incoming contribution per node, and its source.
        let mut best_in = vec![0.0f64; n];
        let mut parent: Vec<Option<usize>> = vec![None; n];
        let mut is_seed = vec![false; n];

        for s in seeds {
            if let Some(&i) = self.index.get(s) {
                let a = self.nodes[i].strength.max(0.0);
                layer[i] += a;
                total[i] += a;
                is_seed[i] = true;
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
                        // Record the dominant synapse into j (strongest single
                        // hop), but never overwrite a seed's own identity.
                        if !is_seed[j] && delta > best_in[j] {
                            best_in[j] = delta;
                            parent[j] = Some(i);
                        }
                    }
                }
            }
            for (t, nx) in total.iter_mut().zip(next.iter()) {
                *t += *nx;
            }
            layer = next;
        }

        let mut out: Vec<(usize, f64)> = (0..n)
            .filter(|&i| total[i] > threshold)
            .map(|i| (i, total[i]))
            .collect();
        out.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        out.into_iter()
            .map(|(i, activation)| Activation {
                uuid: self.nodes[i].uuid,
                activation,
                path: self.trace_path(i, &parent),
            })
            .collect()
    }

    /// Walk the predecessor chain from `node` back to a seed, returning the
    /// labels in seed→node order. Cycle-guarded (a node can appear only once).
    fn trace_path(&self, node: usize, parent: &[Option<usize>]) -> Vec<String> {
        let mut chain = vec![node];
        let mut seen = std::collections::HashSet::from([node]);
        let mut cur = node;
        while let Some(p) = parent[cur] {
            if !seen.insert(p) {
                break;
            }
            chain.push(p);
            cur = p;
        }
        chain.reverse(); // seed first
        chain
            .into_iter()
            .map(|i| self.nodes[i].label.clone())
            .collect()
    }
}

/// One activated memory with its accumulated activation and the dominant
/// synaptic path (`seed → … → this`) as memory labels — recall's "why".
#[derive(Debug, Clone)]
pub struct Activation {
    pub uuid: Uuid,
    pub activation: f64,
    pub path: Vec<String>,
}

/// Elements shared between two small unordered sets (each element yielded once,
/// from `a`'s occurrences).
fn shared<'a, T: PartialEq>(a: &'a [T], b: &[T]) -> impl Iterator<Item = &'a T> {
    a.iter().filter(|x| b.contains(x))
}

/// Return `v` with duplicate elements removed, preserving first-seen order.
/// Keeps anchor sets true per-memory sets so a repeated tag/file/task can't
/// double-count when weighting a shared-anchor edge.
fn dedup<T: Clone + Eq + std::hash::Hash>(v: Vec<T>) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    v.into_iter().filter(|x| seen.insert(x.clone())).collect()
}

/// Document frequency of each anchor: how many memories carry it. Duplicates
/// within a single memory count once, so `df` is a true per-memory count.
fn document_frequencies<T: Clone + Eq + std::hash::Hash>(sets: &[Vec<T>]) -> HashMap<T, usize> {
    let mut df: HashMap<T, usize> = HashMap::new();
    for set in sets {
        let mut seen = std::collections::HashSet::new();
        for v in set {
            if seen.insert(v.clone()) {
                *df.entry(v.clone()).or_insert(0) += 1;
            }
        }
    }
    df
}

// ── Hebbian consolidation ────────────────────────────────────────────────────

/// Group timestamped recall events into co-firing pairs: any two *distinct*
/// memories whose `memory_recalled` events fall within the same `bucket`-wide
/// window fired together. Returns each unordered pair with the number of windows
/// in which they co-fired. Pure (no DB) so it is directly unit-testable.
///
/// Co-firing is a property of the *gap between two recalls*, so windows are
/// grown by single linkage: sort by time and keep extending the current burst
/// while each event sits within `bucket` of the one **before it**, cutting a
/// new burst only on a gap wider than `bucket`.
///
/// Two weaker schemes are wrong here, both in the same way — they impose a grid
/// and lose a genuine co-firing whenever a pair straddles a cell edge:
/// - bucketing on absolute epoch time (`timestamp_millis() / bucket_ms`) splits
///   recalls milliseconds apart that happen to fall either side of a slot;
/// - anchoring each window on its first event merely swaps the epoch grid for
///   an event-derived one: an unrelated *preceding* recall can still push the
///   real pair across the boundary.
///
/// Single linkage can in principle chain a long train of closely-spaced events
/// into one burst; that is the intended reading (sustained activity is one
/// burst), and the `max_bucket` guard below discards any burst too wide to be
/// genuine co-activation.
pub fn coactivation_pairs(
    events: &[(Uuid, DateTime<Utc>)],
    bucket: Duration,
    max_bucket: usize,
) -> Vec<(Uuid, Uuid, u32)> {
    if events.is_empty() || bucket <= Duration::zero() {
        return vec![];
    }
    let bucket_ms = bucket.num_milliseconds().max(1);

    let mut sorted: Vec<&(Uuid, DateTime<Utc>)> = events.iter().collect();
    sorted.sort_by_key(|(_, at)| *at);

    // Sweep the sorted events, cutting a new burst whenever this event sits
    // more than `bucket` after its immediate predecessor.
    let mut windows: Vec<Vec<Uuid>> = Vec::new();
    let mut current: Vec<Uuid> = Vec::new();
    let mut prev: Option<DateTime<Utc>> = None;
    for (u, at) in sorted {
        match prev {
            Some(p) if (*at - p).num_milliseconds() <= bucket_ms => {
                current.push(*u);
            }
            _ => {
                if !current.is_empty() {
                    windows.push(std::mem::take(&mut current));
                }
                current.push(*u);
            }
        }
        prev = Some(*at);
    }
    if !current.is_empty() {
        windows.push(current);
    }

    let mut pair_counts: HashMap<(Uuid, Uuid), u32> = HashMap::new();
    for members in &windows {
        // Distinct memories in this window.
        let mut uniq: Vec<Uuid> = members.clone();
        uniq.sort_unstable();
        uniq.dedup();
        // Bulk-recall guard: a bucket with more distinct memories than
        // `max_bucket` is a listing (e.g. `recall --tag` returning many
        // memories at once), not genuine co-firing. Skip it so a single dump
        // can't record O(k²) spurious synapses. `max_bucket == 0` disables the
        // guard (pair everything).
        if max_bucket > 0 && uniq.len() > max_bucket {
            continue;
        }
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

    let mut out: Vec<(Uuid, Uuid, u32)> = pair_counts
        .into_iter()
        .map(|((a, b), c)| (a, b, c))
        .collect();
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
    max_bucket: usize,
) -> Result<usize> {
    let cutoff = Utc::now() - Duration::days(window_days.max(0));
    let events = db::memory_recall_events_since(conn, &cutoff)?;
    let pairs = coactivation_pairs(&events, bucket, max_bucket);
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
        // IDF-weighted: 'auth' is on 2 of 3 memories, so the edge is the base
        // tag weight scaled by idf(df=2, n=3) — positive but below the raw base.
        let idf = (3.0_f64 / 2.0).ln() / 3.0_f64.ln();
        let expected = W_SHARED_TAG * idf;
        assert!((g.edge_weight(&a, &b).unwrap() - expected).abs() < 1e-9);
        assert!(expected > 0.0 && expected < W_SHARED_TAG);
        assert_eq!(g.edge_weight(&a, &c), None);
    }

    #[test]
    fn duplicate_anchor_within_a_memory_does_not_inflate_edge() {
        let conn = db::open_in_memory_for_test();
        // `a` carries the same tag twice; `c` keeps df(auth)=2 over n=3 so the
        // IDF matches `shared_tag_creates_a_weighted_edge`. The duplicated tag
        // must not double the edge weight.
        let a = seed(&conn, &["auth", "auth"]);
        let b = seed(&conn, &["auth"]);
        let _c = seed(&conn, &["billing"]);

        let g = MemoryGraph::build(&conn).unwrap();
        let idf = (3.0_f64 / 2.0).ln() / 3.0_f64.ln();
        let expected = W_SHARED_TAG * idf; // single contribution, not doubled
        assert!((g.edge_weight(&a, &b).unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn explicit_link_and_shared_anchor_sum() {
        let conn = db::open_in_memory_for_test();
        let a = seed(&conn, &["auth"]);
        let b = seed(&conn, &["auth"]);
        db::insert_memory_link(&conn, &a.to_string(), &b.to_string(), "similar_to", 1.0).unwrap();

        let g = MemoryGraph::build(&conn).unwrap();
        // Both memories carry 'auth' (df == n), so the tag is ubiquitous and
        // idf → 0: the shared anchor adds nothing and only the explicit
        // similar_to (0.7) remains.
        assert!((g.edge_weight(&a, &b).unwrap() - 0.7).abs() < 1e-9);
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
    fn explained_spread_reconstructs_the_synaptic_path() {
        let conn = db::open_in_memory_for_test();
        // Chain a — b — c via shared tags (a,b share "x"; b,c share "y").
        let a = seed(&conn, &["x"]);
        let b = seed(&conn, &["x", "y"]);
        let c = seed(&conn, &["y"]);

        let g = MemoryGraph::build(&conn).unwrap();
        let explained = g.spread_activation_explained(&[a], 2, 0.6, 1e-6);

        let seed_label = g.nodes[g.index[&a]].label.clone();
        let mid_label = g.nodes[g.index[&b]].label.clone();
        let far_label = g.nodes[g.index[&c]].label.clone();

        // The seed's path is just itself.
        let seed_act = explained.iter().find(|e| e.uuid == a).unwrap();
        assert_eq!(seed_act.path, vec![seed_label.clone()]);

        // The two-hop neighbour's dominant path is a → b → c.
        let far = explained.iter().find(|e| e.uuid == c).unwrap();
        assert_eq!(far.path, vec![seed_label, mid_label, far_label]);
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
    fn rare_shared_anchor_binds_tighter_than_a_ubiquitous_one() {
        let conn = db::open_in_memory_for_test();
        // 'common' tag is on many memories; 'rare' tag only on the a–b pair.
        let a = seed(&conn, &["common", "rare"]);
        let b = seed(&conn, &["rare"]); // shares only the rare tag with a
        let mut hubs = vec![];
        for _ in 0..8 {
            hubs.push(seed(&conn, &["common"])); // share only the ubiquitous tag with a
        }

        let g = MemoryGraph::build(&conn).unwrap();
        let rare_edge = g.edge_weight(&a, &b).unwrap();
        let hub_edge = g.edge_weight(&a, &hubs[0]).unwrap();
        assert!(
            rare_edge > hub_edge,
            "a rare shared anchor must bind tighter than a ubiquitous one (idf): rare={rare_edge} hub={hub_edge}"
        );
    }

    #[test]
    fn bulk_recall_bucket_is_ignored_as_noise() {
        let t0 = Utc::now();
        // A bulk listing: 6 memories all recalled at the same instant — a
        // `recall --tag` dump, not genuine co-firing.
        let ids: Vec<Uuid> = (0..6).map(|_| Uuid::new_v4()).collect();
        let events: Vec<_> = ids.iter().map(|u| (*u, t0)).collect();

        let pairs = coactivation_pairs(&events, Duration::seconds(2), 5);
        assert!(
            pairs.is_empty(),
            "a bucket over the max size must yield no co-firing pairs, got {}",
            pairs.len()
        );
    }

    #[test]
    fn coactivation_pairs_group_within_bucket() {
        // A FIXED instant chosen to straddle a 2s epoch-aligned boundary:
        // 1_700_000_001_900 ms has remainder 1900 mod 2000, so `a` at t0 and
        // `b` at t0+100ms fall in *different* fixed slots. The old
        // `timestamp_millis() / bucket_ms` bucketing therefore missed this
        // co-firing (~5% of runs under `Utc::now()`, hence a flaky test).
        // Windows are now cut relative to the events, so this is deterministic.
        let t0 = DateTime::from_timestamp_millis(1_700_000_001_900).unwrap();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let events = vec![
            (a, t0),
            (b, t0 + Duration::milliseconds(100)), // same window as a
            (c, t0 + Duration::seconds(60)),       // far away — own window
        ];
        let pairs = coactivation_pairs(&events, Duration::seconds(2), 5);
        assert_eq!(pairs.len(), 1);
        let (x, y, count) = pairs[0];
        assert_eq!(count, 1);
        let got = if x < y { (x, y) } else { (y, x) };
        let want = if a < b { (a, b) } else { (b, a) };
        assert_eq!(got, want);
    }

    #[test]
    fn coactivation_is_translation_invariant() {
        // Whether two recalls co-fire must depend only on the gap between them,
        // never on where they happen to land on the epoch grid. Sweep a full
        // bucket's worth of start offsets: every one must find the pair.
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        for offset_ms in 0..2000 {
            let t0 = DateTime::from_timestamp_millis(1_700_000_000_000 + offset_ms).unwrap();
            let events = vec![(a, t0), (b, t0 + Duration::milliseconds(100))];
            let pairs = coactivation_pairs(&events, Duration::seconds(2), 5);
            assert_eq!(
                pairs.len(),
                1,
                "lost the co-firing at start offset {offset_ms}ms"
            );
        }
    }

    #[test]
    fn coactivation_survives_an_unrelated_preceding_recall() {
        // Two recalls 100ms apart co-fire. An *earlier, unrelated* recall must
        // not be able to break that: partitioning into windows anchored on the
        // first event merely swaps the epoch grid for an event-derived one, and
        // still loses the pair whenever the preceding event lands in the last
        // `gap`-wide sliver of the bucket. Co-firing is a property of the gap
        // between two events, so sweep every placement of the preceding recall.
        let x = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let t = DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
        for lead_ms in 0..2000 {
            let events = vec![
                (x, t - Duration::milliseconds(lead_ms)),
                (a, t),
                (b, t + Duration::milliseconds(100)),
            ];
            let pairs = coactivation_pairs(&events, Duration::seconds(2), 5);
            assert!(
                pairs
                    .iter()
                    .any(|(p, q, _)| (*p == a && *q == b) || (*p == b && *q == a)),
                "lost the a/b co-firing when an unrelated recall preceded it by {lead_ms}ms"
            );
        }
    }

    #[test]
    fn coactivation_guard_discards_a_long_chained_burst() {
        // Single linkage can chain a train of closely-spaced events into one
        // wide burst. That must not become O(k²) spurious synapses: the
        // max_bucket guard has to discard it.
        let t = DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
        let events: Vec<(Uuid, DateTime<Utc>)> = (0..40)
            .map(|i| (Uuid::new_v4(), t + Duration::milliseconds(i * 10)))
            .collect();
        let pairs = coactivation_pairs(&events, Duration::seconds(2), 5);
        assert!(
            pairs.is_empty(),
            "a 40-memory chained burst is a bulk listing, not co-firing; got {} pairs",
            pairs.len()
        );
        // With the guard disabled the same burst does pair up, proving the
        // events really did chain into one window rather than being dropped.
        let unguarded = coactivation_pairs(&events, Duration::seconds(2), 0);
        assert_eq!(unguarded.len(), 40 * 39 / 2);
    }

    #[test]
    fn coactivation_splits_events_beyond_the_window() {
        // The converse: a gap wider than the bucket must never co-fire, no
        // matter how the pair sits relative to the epoch grid.
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        for offset_ms in 0..500 {
            let t0 = DateTime::from_timestamp_millis(1_700_000_000_000 + offset_ms).unwrap();
            let events = vec![(a, t0), (b, t0 + Duration::milliseconds(2001))];
            let pairs = coactivation_pairs(&events, Duration::seconds(2), 5);
            assert!(
                pairs.is_empty(),
                "spurious co-firing at start offset {offset_ms}ms"
            );
        }
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

        let reinforced = consolidate(&conn, 30, Duration::seconds(2), 0.1, 5).unwrap();
        assert_eq!(reinforced, 1);

        // The learned edge now exists in the graph despite no shared anchor.
        let g = MemoryGraph::build(&conn).unwrap();
        assert!(
            g.edge_weight(&a, &b).is_some(),
            "co-activation should have wired a and b together"
        );
    }

    #[test]
    fn consolidate_still_sees_a_spread_surfaced_memory() {
        // A memory that only ever *surfaced* via spreading activation (never a
        // deliberate recall) must still participate in Hebbian co-activation —
        // separating it from strength must not blind consolidation to it.
        let conn = db::open_in_memory_for_test();
        let a = seed(&conn, &["x"]);
        let b = seed(&conn, &["y"]);

        for _ in 0..2 {
            db::record_memory_recall(&conn, &a).unwrap(); // deliberate seed hit
            db::record_memory_surfaced(&conn, &b).unwrap(); // uninvited spread hit
        }

        let reinforced = consolidate(&conn, 30, Duration::seconds(2), 0.1, 5).unwrap();
        assert_eq!(
            reinforced, 1,
            "the surfaced memory must co-fire with the seed"
        );

        let g = MemoryGraph::build(&conn).unwrap();
        assert!(
            g.edge_weight(&a, &b).is_some(),
            "a spread-surfaced memory must still wire a co-activation edge"
        );
    }
}
