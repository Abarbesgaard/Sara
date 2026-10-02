use rusqlite::Connection;

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

pub(in crate::commands::recall) fn collapse_clusters(
    conn: &Connection,
    hits: Vec<Hit>,
) -> Vec<Hit> {
    let mut kept: Vec<Hit> = Vec::with_capacity(hits.len());
    let mut rep_of: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    let mut collapsed: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    let mut nearest: std::collections::HashMap<(String, String), String> =
        std::collections::HashMap::new();

    for h in hits {
        match family_key(&h) {
            None => kept.push(h),
            Some(fam) => {
                let key = (fam.clone(), hit_project_sig(conn, &h));
                match rep_of.get(&key).copied() {
                    None => {
                        rep_of.insert(key, kept.len());
                        kept.push(h);
                    }
                    Some(idx) => {
                        *collapsed.entry(key.clone()).or_insert(0) += 1;
                        if h.label == fam && kept[idx].label != fam {
                            // The displaced representative was the family's
                            // top-ranked hit, so it outranks any sibling
                            // folded before it.
                            let displaced = std::mem::replace(&mut kept[idx], h);
                            nearest.insert(key, displaced.label);
                        } else {
                            nearest.entry(key).or_insert(h.label);
                        }
                    }
                }
            }
        }
    }

    for h in kept.iter_mut() {
        if let Some(fam) = family_key(h) {
            let key = (fam.clone(), hit_project_sig(conn, h));
            let size = family_size(conn, h, &fam);
            if size > 1 {
                h.cluster = Some(ClusterInfo {
                    canonical_label: fam.clone(),
                    size,
                    collapsed_here: collapsed.get(&key).copied().unwrap_or(0),
                    nearest: nearest.get(&key).cloned(),
                });
            }
        }
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
