use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

use super::types::{Corpus, MemInfo};
use crate::commands::shared::short_handle;
use crate::infrastructure::db;

pub(super) fn load(conn: &Connection, project: Option<&str>) -> Result<Corpus> {
    let linked: HashSet<(String, String)> = db::all_memory_links(conn)?
        .iter()
        .flat_map(|l| {
            [
                (l.from_uuid.clone(), l.to_uuid.clone()),
                (l.to_uuid.clone(), l.from_uuid.clone()),
            ]
        })
        .collect();

    let vectors: HashMap<String, Vec<f32>> = db::active_embeddings(conn)?.into_iter().collect();

    let mut infos = Vec::new();
    for m in &db::list_memories(conn)? {
        if let Some(scope) = project {
            let projects = db::get_item_projects(conn, &m.uuid).unwrap_or_default();
            if !projects.iter().any(|p| p.eq_ignore_ascii_case(scope)) {
                continue;
            }
        }
        infos.push(MemInfo {
            uuid: m.uuid.to_string(),
            label: short_handle(m),
            body: m.body.clone(),
            files: db::get_item_files(conn, &m.uuid).unwrap_or_default(),
            tags: m.tags.iter().map(|t| t.to_lowercase()).collect(),
        });
    }

    Ok(Corpus {
        infos,
        linked,
        vectors,
    })
}
