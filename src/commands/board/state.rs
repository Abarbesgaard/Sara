use std::collections::{HashMap, VecDeque};

use anyhow::Result;
use rusqlite::Connection;
use uuid::Uuid;

use crate::infrastructure::db;
use crate::infrastructure::model::{Status, Task};

use super::types::{BoardState, Feature};

/// Load tasks for the project and group them into features (dependency chains).
pub(super) fn build_state(conn: &Connection, project: String) -> Result<BoardState> {
    let all = db::list_tasks_for_board(conn, &project)?;
    let edges = db::dependency_edges_for_project(conn, &project)?;

    let pos: HashMap<Uuid, usize> = all.iter().enumerate().map(|(i, t)| (t.uuid, i)).collect();
    let n = all.len();

    // Build directed adjacency (blocker → dependents) and undirected neighbors for BFS.
    let mut dependents: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut indeg = vec![0usize; n];
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (task, dep) in &edges {
        if let (Some(&ti), Some(&di)) = (pos.get(task), pos.get(dep)) {
            dependents.entry(di).or_default().push(ti);
            indeg[ti] += 1;
            neighbors[di].push(ti);
            neighbors[ti].push(di);
        }
    }

    // Find connected components via BFS.
    let mut visited = vec![false; n];
    let mut components: Vec<Vec<usize>> = Vec::new();
    for start in 0..n {
        if visited[start] {
            continue;
        }
        let mut comp = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);
        visited[start] = true;
        while let Some(node) = queue.pop_back() {
            comp.push(node);
            for &nb in &neighbors[node] {
                if !visited[nb] {
                    visited[nb] = true;
                    queue.push_back(nb);
                }
            }
        }
        components.push(comp);
    }

    // Split into multi-task features and standalone singletons.
    let mut features_nodes: Vec<Vec<usize>> = Vec::new();
    let mut ungrouped: Vec<usize> = Vec::new();
    for comp in components {
        if comp.len() >= 2 {
            features_nodes.push(topo_order(&comp, &dependents, &indeg));
        } else {
            ungrouped.push(comp[0]);
        }
    }

    // Sort features: active (has pending) first, then by highest pending urgency.
    features_nodes.sort_by(|a, b| {
        sort_key(b, &all)
            .partial_cmp(&sort_key(a, &all))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ungrouped.sort_unstable();

    // Flatten into render order, tagging each task with its feature index.
    let mut tasks: Vec<Task> = Vec::with_capacity(n);
    let mut feature_of: Vec<usize> = Vec::with_capacity(n);
    let mut features: Vec<Feature> = Vec::new();

    for nodes in &features_nodes {
        let fi = features.len();
        let done = nodes.iter().filter(|&&i| all[i].status == Status::Completed).count();
        let title = nodes
            .last()
            .map(|&i| truncate(&all[i].description, 56))
            .unwrap_or_else(|| format!("Feature {}", fi + 1));
        features.push(Feature { title, done, total: nodes.len(), grouped: true });
        for &i in nodes {
            tasks.push(all[i].clone());
            feature_of.push(fi);
        }
    }

    if !ungrouped.is_empty() {
        let fi = features.len();
        let done = ungrouped.iter().filter(|&&i| all[i].status == Status::Completed).count();
        features.push(Feature {
            title: "Standalone tasks".to_string(),
            done,
            total: ungrouped.len(),
            grouped: false,
        });
        for &i in &ungrouped {
            tasks.push(all[i].clone());
            feature_of.push(fi);
        }
    }

    let pending = tasks.iter().filter(|t| t.status == Status::Pending).count();
    let feature_count = features.iter().filter(|f| f.grouped).count();

    Ok(BoardState {
        project,
        done: tasks.len() - pending,
        pending,
        feature_count,
        tasks,
        feature_of,
        features,
        selected: 0,
        scroll: 0,
    })
}

/// Topological sort (blockers first). Ties broken by original position for stable output.
fn topo_order(
    nodes: &[usize],
    dependents: &HashMap<usize, Vec<usize>>,
    global_indeg: &[usize],
) -> Vec<usize> {
    let mut indeg: HashMap<usize, usize> =
        nodes.iter().map(|&i| (i, global_indeg[i])).collect();
    let mut out = Vec::with_capacity(nodes.len());
    while out.len() < nodes.len() {
        let mut ready: Vec<usize> =
            indeg.iter().filter(|&(_, &d)| d == 0).map(|(&i, _)| i).collect();
        if ready.is_empty() {
            break; // cycle guard (shouldn't happen — graph is acyclic)
        }
        ready.sort_unstable();
        for i in ready {
            indeg.remove(&i);
            out.push(i);
            if let Some(deps) = dependents.get(&i) {
                for &j in deps {
                    if let Some(d) = indeg.get_mut(&j) {
                        *d = d.saturating_sub(1);
                    }
                }
            }
        }
    }
    out
}

/// Active-first sort key: (has_pending, best_urgency). Compared descending in the caller.
fn sort_key(nodes: &[usize], all: &[Task]) -> (bool, f64) {
    nodes
        .iter()
        .filter_map(|&i| (all[i].status == Status::Pending).then_some(all[i].urgency))
        .fold((false, 0.0), |(_, best), u| (true, best.max(u)))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}
