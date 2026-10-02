use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

use crate::infrastructure::db;

pub(in crate::commands::reflect) fn projects_by_uuid(
    conn: &Connection,
) -> Result<HashMap<String, HashSet<String>>> {
    let mut map: HashMap<String, HashSet<String>> = HashMap::new();
    for (uuid, project) in db::all_item_projects(conn)? {
        map.entry(uuid).or_default().insert(project);
    }
    Ok(map)
}

pub(in crate::commands::reflect) fn pair_key(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

pub(in crate::commands::reflect) fn deliberate_links(
    conn: &Connection,
) -> Result<HashSet<(String, String)>> {
    let mut set = HashSet::new();
    for link in db::all_memory_links(conn).unwrap_or_default() {
        if link.relation == "co_activated" {
            continue;
        }
        set.insert(pair_key(&link.from_uuid, &link.to_uuid));
    }
    Ok(set)
}

pub(in crate::commands::reflect) fn shares_project(
    a: &str,
    b: &str,
    projects_by_uuid: &HashMap<String, HashSet<String>>,
) -> bool {
    match (projects_by_uuid.get(a), projects_by_uuid.get(b)) {
        (Some(pa), Some(pb)) => pa.intersection(pb).next().is_some(),
        (None, None) => true,
        _ => false,
    }
}
