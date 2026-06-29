use anyhow::Result;
use rusqlite::Connection;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use uuid::Uuid;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::{Status, Task};
use crate::infrastructure::project::detect_current_project;
use crate::infrastructure::tui;

mod render;

pub(super) enum BoardAction {
    Quit,
    OpenTask(String),
}

/// A feature = a chain of tasks linked by `sara dep` dependencies (one connected
/// component of the dependency graph). Standalone tasks land in a trailing
/// pseudo-feature with `grouped == false`.
pub(super) struct Feature {
    pub(super) title: String,
    pub(super) done: usize,
    pub(super) total: usize,
    pub(super) grouped: bool,
}

pub(super) struct BoardState {
    pub(super) project: String,
    /// Tasks in feature-grouped, dependency (blockers-first) order.
    pub(super) tasks: Vec<Task>,
    /// Feature index for each task in `tasks`.
    pub(super) feature_of: Vec<usize>,
    pub(super) features: Vec<Feature>,
    pub(super) selected: usize,
    pub(super) scroll: u16,
}

pub fn run(conn: &Connection, cfg: &Config, project_arg: Option<&str>) -> Result<()> {
    let project = if let Some(p) = project_arg {
        p.to_string()
    } else {
        let (name, _) = detect_current_project(conn, cfg)?;
        name
    };

    let mut st = build_state(conn, project)?;
    if st.tasks.is_empty() {
        println!("No tasks for project '{}'.", st.project);
        return Ok(());
    }

    loop {
        let mut terminal = tui::init_terminal()?;
        let action = render::board_loop(&mut terminal, &mut st)?;
        tui::restore_terminal()?;

        match action {
            BoardAction::Quit => break,
            BoardAction::OpenTask(uuid) => {
                crate::commands::info::run(conn, cfg, &uuid, false, false, false)?;
                // Reload — status/dependencies may have changed in the detail view.
                let project = std::mem::take(&mut st.project);
                let sel = st.selected;
                st = build_state(conn, project)?;
                if st.tasks.is_empty() {
                    break;
                }
                st.selected = sel.min(st.tasks.len() - 1);
            }
        }
    }
    Ok(())
}

/// Load tasks for the project and group them into features (dependency chains).
fn build_state(conn: &Connection, project: String) -> Result<BoardState> {
    let all = db::list_tasks_for_board(conn, &project)?;
    let edges = db::dependency_edges_for_project(conn, &project)?;

    // uuid -> position in `all`
    let pos: HashMap<Uuid, usize> = all.iter().enumerate().map(|(i, t)| (t.uuid, i)).collect();
    let n = all.len();

    // Union-find over task positions; union the two endpoints of every edge.
    let mut parent: Vec<usize> = (0..n).collect();
    // Execution-order adjacency (blocker -> dependent) + in-degree for topo sort.
    let mut dependents: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut indeg = vec![0usize; n];
    for (task, dep) in &edges {
        if let (Some(&ti), Some(&di)) = (pos.get(task), pos.get(dep)) {
            union(&mut parent, ti, di);
            dependents.entry(di).or_default().push(ti);
            indeg[ti] += 1;
        }
    }

    // Bucket task positions by their connected-component root.
    let mut comps: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        comps.entry(r).or_default().push(i);
    }

    // Split into multi-task features and singleton (ungrouped) tasks.
    let mut features_nodes: Vec<Vec<usize>> = Vec::new();
    let mut ungrouped: Vec<usize> = Vec::new();
    for nodes in comps.into_values() {
        if nodes.len() >= 2 {
            features_nodes.push(topo_order(&nodes, &dependents, &indeg));
        } else {
            ungrouped.push(nodes[0]);
        }
    }

    // Order features: active (has pending work) first, by best pending urgency.
    features_nodes.sort_by(|a, b| {
        feature_sort_key(b, &all)
            .partial_cmp(&feature_sort_key(a, &all))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Singletons keep `all` order: pending by urgency, then completed by end.
    ungrouped.sort_unstable();

    // Flatten into the render order, tagging each task with its feature index.
    let mut tasks: Vec<Task> = Vec::with_capacity(n);
    let mut feature_of: Vec<usize> = Vec::with_capacity(n);
    let mut features: Vec<Feature> = Vec::new();

    for nodes in &features_nodes {
        let fi = features.len();
        let total = nodes.len();
        let done = nodes
            .iter()
            .filter(|&&i| all[i].status == Status::Completed)
            .count();
        // Name the feature after its final (goal) task — last in chain order.
        let title = nodes
            .last()
            .map(|&i| truncate(&all[i].description, 56))
            .unwrap_or_else(|| format!("Feature {}", fi + 1));
        features.push(Feature {
            title,
            done,
            total,
            grouped: true,
        });
        for &i in nodes {
            tasks.push(all[i].clone());
            feature_of.push(fi);
        }
    }

    if !ungrouped.is_empty() {
        let fi = features.len();
        let total = ungrouped.len();
        let done = ungrouped
            .iter()
            .filter(|&&i| all[i].status == Status::Completed)
            .count();
        features.push(Feature {
            title: "Standalone tasks".to_string(),
            done,
            total,
            grouped: false,
        });
        for &i in &ungrouped {
            tasks.push(all[i].clone());
            feature_of.push(fi);
        }
    }

    Ok(BoardState {
        project,
        tasks,
        feature_of,
        features,
        selected: 0,
        scroll: 0,
    })
}

fn feature_sort_key(nodes: &[usize], all: &[Task]) -> (i32, f64) {
    let mut best = f64::MIN;
    let mut any_pending = 0;
    for &i in nodes {
        if all[i].status == Status::Pending {
            any_pending = 1;
            best = best.max(all[i].urgency);
        }
    }
    (any_pending, if any_pending == 1 { best } else { 0.0 })
}

/// Kahn topological sort over the component so blockers come before dependents.
/// Ties are broken by original position (urgency order) for stable output.
fn topo_order(
    nodes: &[usize],
    dependents: &HashMap<usize, Vec<usize>>,
    indeg: &[usize],
) -> Vec<usize> {
    use std::collections::HashSet;
    let set: HashSet<usize> = nodes.iter().copied().collect();
    let mut remaining: HashMap<usize, usize> = nodes.iter().map(|&i| (i, indeg[i])).collect();
    // Min-heap on position => earliest/most-urgent ready node first.
    let mut ready: BinaryHeap<Reverse<usize>> = remaining
        .iter()
        .filter(|&(_, &d)| d == 0)
        .map(|(&i, _)| Reverse(i))
        .collect();

    let mut out = Vec::with_capacity(nodes.len());
    while let Some(Reverse(i)) = ready.pop() {
        out.push(i);
        if let Some(deps) = dependents.get(&i) {
            for &j in deps {
                if !set.contains(&j) {
                    continue;
                }
                if let Some(d) = remaining.get_mut(&j) {
                    *d -= 1;
                    if *d == 0 {
                        ready.push(Reverse(j));
                    }
                }
            }
        }
    }
    // Any nodes left (shouldn't happen — graph is acyclic) appended in position order.
    if out.len() < nodes.len() {
        let mut leftover: Vec<usize> = nodes.iter().copied().filter(|i| !out.contains(i)).collect();
        leftover.sort_unstable();
        out.extend(leftover);
    }
    out
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]]; // path halving
        x = parent[x];
    }
    x
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        parent[ra] = rb;
    }
}

pub(super) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}
