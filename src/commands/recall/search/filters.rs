use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashSet;

use crate::infrastructure::db;

pub(in crate::commands::recall) fn exact_uuids(
    conn: &Connection,
    tags: &[String],
    projects: &[String],
    files: &[String],
) -> Result<Option<HashSet<uuid::Uuid>>> {
    let file_uuids: Option<HashSet<uuid::Uuid>> = if !files.is_empty() {
        let mut combined: Option<HashSet<uuid::Uuid>> = None;
        for path in files {
            let prefix = path.ends_with('/');
            let items = db::find_items_by_file(conn, path, prefix)?;
            let uuids: HashSet<uuid::Uuid> = items.into_iter().map(|i| i.uuid).collect();
            combined = Some(match combined {
                Some(existing) => existing.intersection(&uuids).copied().collect(),
                None => uuids,
            });
        }
        combined
    } else {
        None
    };

    let tag_project_uuids: Option<HashSet<uuid::Uuid>> = if !tags.is_empty() || !projects.is_empty()
    {
        let mut by_tag: Option<HashSet<uuid::Uuid>> = None;
        for tag in tags {
            let hit: HashSet<uuid::Uuid> = db::find_items_by_tag(conn, tag)?
                .into_iter()
                .map(|i| i.uuid)
                .collect();
            by_tag = Some(match by_tag {
                Some(existing) => existing.intersection(&hit).copied().collect(),
                None => hit,
            });
        }

        let mut by_project: Option<HashSet<uuid::Uuid>> = None;
        for project in projects {
            let mut hit: HashSet<uuid::Uuid> = db::find_items_by_project(conn, project)?
                .into_iter()
                .map(|i| i.uuid)
                .collect();
            hit.extend(db::find_cross_project_canonicals_for_project(
                conn, project,
            )?);
            by_project = Some(match by_project {
                Some(mut existing) => {
                    existing.extend(hit);
                    existing
                }
                None => hit,
            });
        }

        Some(match (by_tag, by_project) {
            (Some(t), Some(p)) => t.intersection(&p).copied().collect(),
            (Some(t), None) => t,
            (None, Some(p)) => p,
            (None, None) => HashSet::new(),
        })
    } else {
        None
    };

    Ok(match (file_uuids, tag_project_uuids) {
        (Some(f), Some(tp)) => Some(f.intersection(&tp).copied().collect()),
        (Some(f), None) => Some(f),
        (None, Some(tp)) => Some(tp),
        (None, None) => None,
    })
}
