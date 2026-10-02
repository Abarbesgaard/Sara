use rusqlite::Connection;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::commands::recall::types::{ClusterInfo, Hit};
use crate::commands::shared::derived_count;
use crate::infrastructure::db;

pub(in crate::commands::recall) fn family_key(h: &Hit) -> Option<String> {
    if let Some(parent) = h.derived_from_labels.first() {
        Some(parent.clone())
    } else if !h.derived_children.is_empty() {
        Some(h.label.clone())
    } else {
        None
    }
}

pub(in crate::commands::recall) fn family_size(
    conn: &Connection,
    rep: &Hit,
    canonical_label: &str,
) -> usize {
    if rep.label == canonical_label {
        return 1 + rep.derived_children.len();
    }
    match db::get_item_by_handle(conn, canonical_label) {
        Ok(canon) => 1 + derived_count(conn, &canon.uuid.to_string()),
        Err(_) => 1,
    }
}

struct Family {
    rep: usize,
    collapsed: usize,
    nearest: Option<String>,
}

pub(in crate::commands::recall) fn collapse_clusters(
    conn: &Connection,
    hits: Vec<Hit>,
) -> Vec<Hit> {
    let mut kept: Vec<Hit> = Vec::with_capacity(hits.len());
    let mut families: HashMap<(String, String), Family> = HashMap::new();

    for h in hits {
        let Some(fam) = family_key(&h) else {
            kept.push(h);
            continue;
        };
        let sig = hit_project_sig(conn, &h);
        match families.entry((fam, sig)) {
            Entry::Vacant(v) => {
                v.insert(Family {
                    rep: kept.len(),
                    collapsed: 0,
                    nearest: None,
                });
                kept.push(h);
            }
            Entry::Occupied(o) => {
                let canonical = &o.key().0;
                let promote = h.label == *canonical && kept[o.get().rep].label != *canonical;
                let family = o.into_mut();
                family.collapsed += 1;
                if promote {
                    let displaced = std::mem::replace(&mut kept[family.rep], h);
                    family.nearest = Some(displaced.label);
                } else if family.nearest.is_none() {
                    family.nearest = Some(h.label);
                }
            }
        }
    }

    for h in kept.iter_mut() {
        let Some(fam) = family_key(h) else { continue };
        let size = family_size(conn, h, &fam);
        if size <= 1 {
            continue;
        }
        let key = (fam, hit_project_sig(conn, h));
        let family = families.get(&key);
        let collapsed_here = family.map_or(0, |f| f.collapsed);
        let nearest = family.and_then(|f| f.nearest.clone());
        h.cluster = Some(ClusterInfo {
            canonical_label: key.0,
            size,
            collapsed_here,
            nearest,
        });
    }

    kept
}

pub(in crate::commands::recall) fn hit_project_sig(conn: &Connection, h: &Hit) -> String {
    match h.item_uuid {
        Some(u) => {
            let mut ps = db::get_item_projects(conn, &u).unwrap_or_default();
            ps.sort();
            ps.join("\u{1f}")
        }
        None => String::new(),
    }
}
