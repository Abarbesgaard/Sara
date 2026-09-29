use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::infrastructure::db;

const W_SHARED_TASK: f64 = 0.8;
const W_SHARED_FILE: f64 = 0.6;
const W_SHARED_TAG: f64 = 0.3;

fn relation_weight(relation: &str, stored: f64) -> f64 {
    let base = match relation {
        "derived_from" => 0.8,
        "similar_to" => 0.7,
        "used_in" => 0.6,
        "supersedes" => 0.5,
        "co_activated" => return (0.25 * stored).min(MAX_EDGE),
        _ => 0.4,
    };
    (base * stored).min(MAX_EDGE)
}

const MAX_EDGE: f64 = 1.0;

const INVERT_OVERHEAD: u128 = 4;

#[derive(Debug, Clone)]
pub struct Node {
    pub uuid: Uuid,
    pub label: String,
    pub strength: f64,
}

#[derive(Debug, Default)]
pub struct MemoryGraph {
    pub nodes: Vec<Node>,
    index: HashMap<Uuid, usize>,
    adj: Vec<Vec<(usize, f64)>>,
}

impl MemoryGraph {
    pub fn build(conn: &Connection) -> Result<MemoryGraph> {
        let memories = db::list_memories(conn)?;

        let mut nodes = Vec::with_capacity(memories.len());
        let mut index = HashMap::with_capacity(memories.len());
        let mut tag_sets: Vec<Vec<String>> = Vec::with_capacity(memories.len());
        let mut file_sets: Vec<Vec<String>> = Vec::with_capacity(memories.len());
        let mut task_sets: Vec<Vec<Uuid>> = Vec::with_capacity(memories.len());

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
            tag_sets.push(dedup(m.tags.clone()));
            file_sets.push(dedup(all_files.remove(&m.uuid).unwrap_or_default()));
            task_sets.push(dedup(all_tasks.remove(&m.uuid).unwrap_or_default()));
        }

        let mut edges: HashMap<(usize, usize), f64> = HashMap::new();
        let add = |a: usize, b: usize, w: f64, edges: &mut HashMap<(usize, usize), f64>| {
            if a == b || w <= 0.0 {
                return;
            }
            let key = if a < b { (a, b) } else { (b, a) };
            let e = edges.entry(key).or_insert(0.0);
            *e = (*e + w).min(MAX_EDGE);
        };

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

        let n = nodes.len();
        let tag_post = postings(&tag_sets);
        let file_post = postings(&file_sets);
        let task_post = postings(&task_sets);

        let ln_n = (n as f64).ln();
        let idf = |df: usize| -> f64 {
            if n < 2 || df == 0 || df >= n {
                return 0.0;
            }
            (n as f64 / df as f64).ln() / ln_n
        };

        let score = |i: usize, j: usize| -> f64 {
            let mut w = 0.0;
            for t in shared(&tag_sets[i], &tag_sets[j]) {
                w += W_SHARED_TAG * idf(tag_post.get(t).map_or(0, Vec::len));
            }
            for f in shared(&file_sets[i], &file_sets[j]) {
                w += W_SHARED_FILE * idf(file_post.get(f).map_or(0, Vec::len));
            }
            for t in shared(&task_sets[i], &task_sets[j]) {
                w += W_SHARED_TASK * idf(task_post.get(t).map_or(0, Vec::len));
            }
            w
        };

        let inverted_pairs = pair_estimate(&tag_post, n)
            + pair_estimate(&file_post, n)
            + pair_estimate(&task_post, n);
        let dense_pairs = (n as u128) * (n.saturating_sub(1) as u128) / 2;

        if inverted_pairs.saturating_mul(INVERT_OVERHEAD) < dense_pairs {
            let mut candidates: HashSet<(usize, usize)> = HashSet::new();
            collect_pairs(&tag_post, n, &mut candidates);
            collect_pairs(&file_post, n, &mut candidates);
            collect_pairs(&task_post, n, &mut candidates);

            let mut pairs: Vec<(usize, usize)> = candidates.into_iter().collect();
            pairs.sort_unstable();
            for (i, j) in pairs {
                let w = score(i, j);
                if w > 0.0 {
                    add(i, j, w, &mut edges);
                }
            }
        } else {
            for i in 0..n {
                for j in (i + 1)..n {
                    let w = score(i, j);
                    if w > 0.0 {
                        add(i, j, w, &mut edges);
                    }
                }
            }
        }

        let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
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

    pub fn edge_weight(&self, a: &Uuid, b: &Uuid) -> Option<f64> {
        let (&ia, &ib) = (self.index.get(a)?, self.index.get(b)?);
        self.adj[ia].iter().find(|(j, _)| *j == ib).map(|(_, w)| *w)
    }

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
        chain.reverse();
        chain
            .into_iter()
            .map(|i| self.nodes[i].label.clone())
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct Activation {
    pub uuid: Uuid,
    pub activation: f64,
    pub path: Vec<String>,
}

fn shared<'a, T: PartialEq>(a: &'a [T], b: &'a [T]) -> impl Iterator<Item = &'a T> {
    a.iter().filter(move |x| b.contains(x))
}

fn dedup<T: Clone + Eq + std::hash::Hash>(v: Vec<T>) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    v.into_iter().filter(|x| seen.insert(x.clone())).collect()
}

fn postings<T: Clone + Eq + std::hash::Hash>(sets: &[Vec<T>]) -> HashMap<T, Vec<usize>> {
    let mut post: HashMap<T, Vec<usize>> = HashMap::new();
    for (i, set) in sets.iter().enumerate() {
        for v in set {
            post.entry(v.clone()).or_default().push(i);
        }
    }
    post
}

fn pair_estimate<T>(post: &HashMap<T, Vec<usize>>, n: usize) -> u128 {
    post.values()
        .map(|members| {
            let df = members.len();
            if df < 2 || df >= n {
                0
            } else {
                (df as u128) * (df as u128 - 1) / 2
            }
        })
        .sum()
}

fn collect_pairs<T>(post: &HashMap<T, Vec<usize>>, n: usize, out: &mut HashSet<(usize, usize)>) {
    for members in post.values() {
        let df = members.len();
        if df < 2 || df >= n {
            continue;
        }
        for (a, &i) in members.iter().enumerate() {
            for &j in &members[a + 1..] {
                out.insert(if i < j { (i, j) } else { (j, i) });
            }
        }
    }
}

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
        let mut uniq: Vec<Uuid> = members.clone();
        uniq.sort_unstable();
        uniq.dedup();
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

pub fn consolidate(
    conn: &Connection,
    window_days: i64,
    bucket: Duration,
    delta: f64,
    max_bucket: usize,
) -> Result<usize> {
    let cutoff = Utc::now() - Duration::days(window_days.max(0));
    let events = db::memory_recall_events_since(conn, &cutoff)?;
    let edges: Vec<(String, String, f64)> = coactivation_pairs(&events, bucket, max_bucket)
        .into_iter()
        .map(|(a, b, count)| (a.to_string(), b.to_string(), delta * count as f64))
        .collect();
    db::replace_coactivations(conn, &edges)?;
    Ok(edges.len())
}

#[cfg(test)]
#[path = "../../tests/unit/infrastructure/memory_graph.rs"]
mod tests;
