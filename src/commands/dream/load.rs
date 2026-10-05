use std::collections::HashMap;

use anyhow::Result;
use rusqlite::Connection;

use crate::commands::shared::item_label;
use crate::infrastructure::db;
use crate::infrastructure::memory::graph::MemoryGraph;

use super::layout::{force_layout, graph_edges, spring_stiffness};
use super::types::{Assoc, Bond, DreamData, Neighbor, NodeKind, Star, WebData};

pub(super) fn load(conn: &Connection, handle: &str) -> Result<DreamData> {
    let item = db::get_item_by_handle(conn, handle)?;
    if item.kind != "memory" {
        anyhow::bail!(
            "`sara dream` peeks into memories — {handle} is a {}",
            item.kind
        );
    }
    let label = item_label(&item);
    let strength = db::item_strength(conn, &item);
    let files = db::get_item_files(conn, &item.uuid).unwrap_or_default();

    let mut neighbors = vec![];
    let uuid_str = item.uuid.to_string();
    for link in db::get_memory_links_from(conn, &uuid_str).unwrap_or_default() {
        if let Ok(other) = db::get_item_by_uuid(conn, &link.to_uuid) {
            neighbors.push(Neighbor {
                kind: NodeKind::Memory {
                    label: item_label(&other),
                    relation: link.relation,
                },
            });
        }
    }
    for link in db::get_memory_links_to(conn, &uuid_str).unwrap_or_default() {
        if let Ok(other) = db::get_item_by_uuid(conn, &link.from_uuid) {
            neighbors.push(Neighbor {
                kind: NodeKind::Memory {
                    label: item_label(&other),
                    relation: format!("⟵ {}", link.relation),
                },
            });
        }
    }
    for (task, source) in db::get_item_task_links(conn, &item.uuid).unwrap_or_default() {
        neighbors.push(Neighbor {
            kind: NodeKind::Task {
                id: task.id.map(|i| i.to_string()).unwrap_or_else(|| "?".into()),
                desc: task.description.clone(),
                source,
            },
        });
    }
    for f in &files {
        let name = f.rsplit('/').next().unwrap_or(f).to_string();
        neighbors.push(Neighbor {
            kind: NodeKind::File { name },
        });
    }

    let sparkline = db::memory_recall_daily_counts(conn, &item.uuid, 30);
    let recall_total_30d = sparkline.iter().sum();
    Ok(DreamData {
        provisional: item.status == "provisional",
        label,
        strength,
        files,
        neighbors,
        sparkline,
        recall_total_30d,
        item,
    })
}

pub(super) fn open(conn: &Connection, handle: &str) -> Result<DreamData> {
    let data = load(conn, handle)?;
    let _ = db::record_memory_recall(conn, &data.item.uuid);
    Ok(data)
}

pub(super) fn navigate_back(conn: &Connection, breadcrumb: &mut Vec<String>) -> Option<DreamData> {
    while let Some(prev) = breadcrumb.pop() {
        if let Ok(d) = load(conn, &prev) {
            return Some(d);
        }
    }
    None
}

pub(super) fn load_web(conn: &Connection) -> Result<WebData> {
    let memories = db::list_memories(conn)?;
    let strengths = db::item_strengths(conn, &memories);
    let recent_counts = db::memory_recall_daily_counts_all(conn, 7);
    let mut index = HashMap::new();
    let mut stars: Vec<Star> = memories
        .iter()
        .enumerate()
        .map(|(i, m)| {
            index.insert(m.uuid.to_string(), i);
            let recent: u64 = recent_counts.get(&m.uuid).map_or(0, |c| c.iter().sum());
            let label = item_label(m);
            let haystack = format!(
                "{} {} {} {}",
                label.to_lowercase(),
                m.tags.join(" ").to_lowercase(),
                m.title.to_lowercase(),
                m.body.to_lowercase()
            );
            Star {
                label,
                title: m.title.clone(),
                strength: strengths.get(&m.uuid).copied().unwrap_or(1.0),
                provisional: m.status == "provisional",
                tags: m.tags.clone(),
                haystack,
                x: 0.0,
                y: 0.0,
                recently_recalled: recent > 0,
            }
        })
        .collect();
    let bonds: Vec<Bond> = db::all_memory_links(conn)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|l| {
            Some(Bond {
                a: *index.get(&l.from_uuid)?,
                b: *index.get(&l.to_uuid)?,
                relation: l.relation,
            })
        })
        .collect();
    let edges = MemoryGraph::build(conn)
        .map(|g| graph_edges(&g, &index))
        .unwrap_or_default();
    let affinity: Vec<(usize, usize, f64)> = edges
        .iter()
        .map(|&(a, b, w)| (a, b, spring_stiffness(w)))
        .collect();
    force_layout(&mut stars, &affinity, 250);
    let links: Vec<Assoc> = edges
        .into_iter()
        .map(|(a, b, weight)| Assoc { a, b, weight })
        .collect();
    Ok(WebData {
        stars,
        bonds,
        links,
    })
}
