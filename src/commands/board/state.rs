use std::collections::{HashMap, VecDeque};

use anyhow::Result;
use rusqlite::Connection;
use uuid::Uuid;

use crate::infrastructure::db;
use crate::infrastructure::model::{Status, Task};

use super::types::{BoardState, Feature, GroupMode};

/// Load tasks for the project and group them per `mode` (feature/dependency
/// chains by default, or by linked GitHub issue when toggled).
pub(super) fn build_state(
    conn: &Connection,
    project: String,
    mode: GroupMode,
) -> Result<BoardState> {
    let all = db::list_tasks_for_board(conn, &project)?;

    let (tasks, feature_of, features) = match mode {
        GroupMode::Feature => build_feature_grouping(conn, &project, &all)?,
        GroupMode::Issue => build_issue_grouping(conn, &all)?,
    };

    let flags_by_task = db::link_flags_by_task(conn).unwrap_or_default();
    let badges = tasks
        .iter()
        .map(|t| {
            flags_by_task
                .get(&t.uuid.to_string())
                .copied()
                .unwrap_or_default()
        })
        .collect();

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
        badges,
        mode,
        selected: 0,
        scroll: 0,
    })
}

/// Group by dependency chain (connected component of the `sara dep` graph) —
/// the original/default board grouping.
fn build_feature_grouping(
    conn: &Connection,
    project: &str,
    all: &[Task],
) -> Result<(Vec<Task>, Vec<usize>, Vec<Feature>)> {
    let edges = db::dependency_edges_for_project(conn, project)?;

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
        sort_key(b, all)
            .partial_cmp(&sort_key(a, all))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ungrouped.sort_unstable();

    // Flatten into render order, tagging each task with its feature index.
    let mut tasks: Vec<Task> = Vec::with_capacity(n);
    let mut feature_of: Vec<usize> = Vec::with_capacity(n);
    let mut features: Vec<Feature> = Vec::new();

    for nodes in &features_nodes {
        let fi = features.len();
        let done = nodes
            .iter()
            .filter(|&&i| all[i].status == Status::Completed)
            .count();
        let title = nodes
            .last()
            .map(|&i| truncate(&all[i].description, 56))
            .unwrap_or_else(|| format!("Feature {}", fi + 1));
        features.push(Feature {
            title,
            done,
            total: nodes.len(),
            grouped: true,
        });
        for &i in nodes {
            tasks.push(all[i].clone());
            feature_of.push(fi);
        }
    }

    if !ungrouped.is_empty() {
        let fi = features.len();
        let done = ungrouped
            .iter()
            .filter(|&&i| all[i].status == Status::Completed)
            .count();
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

    Ok((tasks, feature_of, features))
}

/// Group by linked GitHub issue, reusing `group_tasks_by_issue` (already
/// shipped for `sara list --by-issue`) instead of a new query.
fn build_issue_grouping(
    conn: &Connection,
    all: &[Task],
) -> Result<(Vec<Task>, Vec<usize>, Vec<Feature>)> {
    let (groups, ungrouped) = db::group_tasks_by_issue(conn, all)?;

    let mut tasks: Vec<Task> = Vec::with_capacity(all.len());
    let mut feature_of: Vec<usize> = Vec::with_capacity(all.len());
    let mut features: Vec<Feature> = Vec::new();

    for group in &groups {
        let fi = features.len();
        let done = group
            .tasks
            .iter()
            .filter(|t| t.status == Status::Completed)
            .count();
        features.push(Feature {
            title: format!("Issue #{} · {}", group.number, group.owner_repo),
            done,
            total: group.tasks.len(),
            grouped: true,
        });
        for t in &group.tasks {
            tasks.push(t.clone());
            feature_of.push(fi);
        }
    }

    if !ungrouped.is_empty() {
        let fi = features.len();
        let done = ungrouped
            .iter()
            .filter(|t| t.status == Status::Completed)
            .count();
        features.push(Feature {
            title: "No linked issue".to_string(),
            done,
            total: ungrouped.len(),
            grouped: false,
        });
        for t in ungrouped {
            tasks.push(t);
            feature_of.push(fi);
        }
    }

    Ok((tasks, feature_of, features))
}

/// Topological sort (blockers first). Ties broken by original position for stable output.
fn topo_order(
    nodes: &[usize],
    dependents: &HashMap<usize, Vec<usize>>,
    global_indeg: &[usize],
) -> Vec<usize> {
    let mut indeg: HashMap<usize, usize> = nodes.iter().map(|&i| (i, global_indeg[i])).collect();
    let mut out = Vec::with_capacity(nodes.len());
    while out.len() < nodes.len() {
        let mut ready: Vec<usize> = indeg
            .iter()
            .filter(|&(_, &d)| d == 0)
            .map(|(&i, _)| i)
            .collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn task(conn: &Connection, desc: &str) -> Task {
        let mut t = Task::new(desc.to_string(), "tk".to_string());
        db::insert_task(conn, &mut t).unwrap();
        t
    }

    #[test]
    fn feature_mode_groups_dependent_tasks_and_leaves_others_standalone() {
        let conn = db::open_in_memory_for_test();
        let a = task(&conn, "a");
        let b = task(&conn, "b");
        let standalone = task(&conn, "standalone");
        db::add_dependency(&conn, &b.uuid, &a.uuid).unwrap(); // b depends on a

        let st = build_state(&conn, "tk".to_string(), GroupMode::Feature).unwrap();

        assert_eq!(st.mode, GroupMode::Feature);
        assert_eq!(st.tasks.len(), 3);
        // a and b share a feature (grouped); the standalone task doesn't.
        let fi_a = st.feature_of[st.tasks.iter().position(|t| t.uuid == a.uuid).unwrap()];
        let fi_b = st.feature_of[st.tasks.iter().position(|t| t.uuid == b.uuid).unwrap()];
        let fi_standalone = st.feature_of[st
            .tasks
            .iter()
            .position(|t| t.uuid == standalone.uuid)
            .unwrap()];
        assert_eq!(fi_a, fi_b);
        assert_ne!(fi_a, fi_standalone);
        assert!(st.features[fi_a].grouped);
        assert!(!st.features[fi_standalone].grouped);
    }

    #[test]
    fn issue_mode_groups_by_linked_github_issue_and_buckets_the_rest() {
        let conn = db::open_in_memory_for_test();
        let a = task(&conn, "a");
        let b = task(&conn, "b");
        let unlinked = task(&conn, "unlinked");
        db::add_link(&conn, &a.uuid, "https://github.com/o/r/issues/5", None).unwrap();
        db::add_link(&conn, &b.uuid, "https://github.com/o/r/issues/5", None).unwrap();

        let st = build_state(&conn, "tk".to_string(), GroupMode::Issue).unwrap();

        assert_eq!(st.mode, GroupMode::Issue);
        assert_eq!(st.tasks.len(), 3);
        let fi_a = st.feature_of[st.tasks.iter().position(|t| t.uuid == a.uuid).unwrap()];
        let fi_b = st.feature_of[st.tasks.iter().position(|t| t.uuid == b.uuid).unwrap()];
        let fi_unlinked = st.feature_of[st
            .tasks
            .iter()
            .position(|t| t.uuid == unlinked.uuid)
            .unwrap()];
        assert_eq!(fi_a, fi_b);
        assert_ne!(fi_a, fi_unlinked);
        assert!(st.features[fi_a].title.contains("#5"));
        assert_eq!(st.features[fi_unlinked].title, "No linked issue");
    }

    #[test]
    fn badges_reflect_pr_and_issue_links_per_task() {
        let conn = db::open_in_memory_for_test();
        let with_pr = task(&conn, "has a pr");
        let with_issue = task(&conn, "has an issue");
        let with_nothing = task(&conn, "has nothing");
        db::add_link(&conn, &with_pr.uuid, "https://github.com/o/r/pull/9", None).unwrap();
        db::add_link(
            &conn,
            &with_issue.uuid,
            "https://github.com/o/r/issues/3",
            None,
        )
        .unwrap();

        let st = build_state(&conn, "tk".to_string(), GroupMode::Feature).unwrap();

        let badge_of = |uuid: uuid::Uuid| {
            let i = st.tasks.iter().position(|t| t.uuid == uuid).unwrap();
            st.badges[i]
        };
        assert!(badge_of(with_pr.uuid).pr);
        assert!(badge_of(with_issue.uuid).issue);
        assert!(!badge_of(with_nothing.uuid).any);
    }

    #[test]
    fn toggled_flips_between_feature_and_issue() {
        assert_eq!(GroupMode::Feature.toggled(), GroupMode::Issue);
        assert_eq!(GroupMode::Issue.toggled(), GroupMode::Feature);
    }
}
