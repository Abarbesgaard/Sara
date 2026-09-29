use super::super::*;
use crate::infrastructure::model::{Item, Task};
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

pub(crate) fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<Item> {
    let tags_json: String = row.get(6)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(Item {
        uuid: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::new_v4()),
        display_id: row.get(2)?,
        kind: row.get(1)?,
        title: row.get(3)?,
        url: row.get(4)?,
        project: row.get(5)?,
        tags,
        path: Some(row.get(7)?),
        summary: row.get(8)?,
        body: row.get(9)?,
        created: str_to_dt(&row.get::<_, String>(10)?).unwrap_or_else(|_| Utc::now()),
        modified: str_to_dt(&row.get::<_, String>(11)?).unwrap_or_else(|_| Utc::now()),
        status: row.get(12)?,
        source_task_uuid: row
            .get::<_, Option<String>>(13)?
            .and_then(|s| Uuid::parse_str(&s).ok()),
        files: vec![],
        linked_tasks: vec![],
    })
}

fn next_item_display_id(conn: &Connection, kind: &str) -> Result<i64> {
    let max: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(display_id), 0) FROM items WHERE kind = ?1 AND status IN ('active','provisional')",
            [kind],
            |r| r.get(0),
        )
        .unwrap_or(0);
    Ok(max + 1)
}

pub fn insert_item(conn: &Connection, item: &mut Item) -> Result<()> {
    with_write_lock(conn, || {
        if item.display_id.is_none() {
            item.display_id = Some(next_item_display_id(conn, &item.kind)?);
        }
        let path = item
            .path
            .clone()
            .context("item path must be set before insert")?;
        let tags_json = serde_json::to_string(&item.tags)?;
        conn.execute(
            "INSERT INTO items (uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status, source_task_uuid)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            rusqlite::params![
                item.uuid.to_string(),
                item.kind,
                item.display_id,
                item.title,
                item.url,
                item.project,
                tags_json,
                path,
                item.summary,
                item.body,
                dt_to_str(&item.created),
                dt_to_str(&item.modified),
                item.status,
                item.source_task_uuid.map(|u| u.to_string()),
            ],
        )?;
        set_item_tags(conn, &item.uuid, &item.tags)?;
        Ok(())
    })
}

pub fn list_items(conn: &Connection, kind: Option<&str>) -> Result<Vec<Item>> {
    let mut items = vec![];
    if let Some(k) = kind {
        let mut stmt = conn.prepare(
            "SELECT uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status, source_task_uuid
             FROM items WHERE status IN ('active','provisional') AND kind = ?1 ORDER BY display_id",
        )?;
        let rows = stmt.query_map([k], row_to_item)?;
        for r in rows {
            items.push(r?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status, source_task_uuid
             FROM items WHERE status IN ('active','provisional') ORDER BY kind, display_id",
        )?;
        let rows = stmt.query_map([], row_to_item)?;
        for r in rows {
            items.push(r?);
        }
    }
    Ok(items)
}

pub fn upsert_embedding(conn: &Connection, ref_uuid: &str, vector: &[f32]) -> Result<()> {
    let vector_json = serde_json::to_string(vector)?;
    conn.execute(
        "INSERT INTO embeddings (ref_uuid, vector_json) VALUES (?1, ?2)
         ON CONFLICT(ref_uuid) DO UPDATE SET vector_json = excluded.vector_json",
        rusqlite::params![ref_uuid, vector_json],
    )?;
    Ok(())
}

pub fn get_embedding(conn: &Connection, ref_uuid: &str) -> Result<Option<Vec<f32>>> {
    let row: Option<String> = conn
        .query_row(
            "SELECT vector_json FROM embeddings WHERE ref_uuid = ?1",
            [ref_uuid],
            |r| r.get(0),
        )
        .optional()?;
    match row {
        Some(json) => Ok(Some(serde_json::from_str(&json)?)),
        None => Ok(None),
    }
}

pub fn all_embeddings(conn: &Connection) -> Result<Vec<(String, Vec<f32>)>> {
    let mut stmt = conn.prepare("SELECT ref_uuid, vector_json FROM embeddings")?;
    let rows = stmt.query_map([], |r| {
        let uuid: String = r.get(0)?;
        let json: String = r.get(1)?;
        Ok((uuid, json))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (uuid, json) = r?;
        if let Ok(v) = serde_json::from_str::<Vec<f32>>(&json) {
            out.push((uuid, v));
        }
    }
    Ok(out)
}

pub fn delete_embedding(conn: &Connection, ref_uuid: &str) -> Result<()> {
    conn.execute("DELETE FROM embeddings WHERE ref_uuid = ?1", [ref_uuid])?;
    Ok(())
}

pub fn active_embeddings(conn: &Connection) -> Result<Vec<(String, Vec<f32>)>> {
    let mut stmt = conn.prepare(
        "SELECT e.ref_uuid, e.vector_json FROM embeddings e
         JOIN items i ON i.uuid = e.ref_uuid
         WHERE i.status IN ('active','provisional')",
    )?;
    let rows = stmt.query_map([], |r| {
        let uuid: String = r.get(0)?;
        let json: String = r.get(1)?;
        Ok((uuid, json))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (uuid, json) = r?;
        if let Ok(v) = serde_json::from_str::<Vec<f32>>(&json) {
            out.push((uuid, v));
        }
    }
    Ok(out)
}

pub fn list_memories(conn: &Connection) -> Result<Vec<Item>> {
    let mut stmt = conn.prepare(
        "SELECT uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status, source_task_uuid
         FROM items WHERE status IN ('active','provisional') AND kind = 'memory' ORDER BY created DESC",
    )?;
    let rows = stmt.query_map([], row_to_item)?;
    let mut items = vec![];
    for r in rows {
        items.push(r?);
    }
    Ok(items)
}

pub fn get_item_by_handle(conn: &Connection, handle: &str) -> Result<Item> {
    let handle = handle.trim().to_lowercase();
    let (kind, id_str) = if let Some(rest) = handle.strip_prefix('n') {
        ("note", rest)
    } else if let Some(rest) = handle.strip_prefix('l') {
        ("link", rest)
    } else if let Some(rest) = handle.strip_prefix('m') {
        ("memory", rest)
    } else {
        return get_item_by_uuid_prefix(conn, &handle);
    };
    let id: i64 = id_str.parse().context("Invalid item id")?;
    conn.query_row(
        "SELECT uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status, source_task_uuid
         FROM items WHERE kind = ?1 AND display_id = ?2 AND status IN ('active','provisional')",
        rusqlite::params![kind, id],
        row_to_item,
    )
    .map_err(|_| anyhow::anyhow!("No active {kind} with id {id}"))
}

pub fn get_item_by_uuid_prefix(conn: &Connection, prefix: &str) -> Result<Item> {
    let pattern = like_prefix_pattern(prefix);
    let mut stmt = conn.prepare(
        "SELECT uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status, source_task_uuid
         FROM items WHERE uuid LIKE ?1 ESCAPE '\\' AND status IN ('active','provisional') LIMIT 2",
    )?;
    let mut items = stmt
        .query_map([pattern], row_to_item)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if items.len() > 1 {
        return Err(anyhow::anyhow!(
            "Ambiguous item uuid prefix '{prefix}' matches multiple items — use a longer prefix or the mN/nN/lN handle"
        ));
    }
    items
        .pop()
        .ok_or_else(|| anyhow::anyhow!("No active item with handle or uuid matching '{prefix}'"))
}

pub fn get_item_by_uuid(conn: &Connection, uuid: &str) -> Result<Item> {
    conn.query_row(
        "SELECT uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status, source_task_uuid
         FROM items WHERE uuid = ?1 AND status IN ('active','provisional')",
        [uuid],
        row_to_item,
    )
    .map_err(|_| anyhow::anyhow!("No active item with uuid {uuid}"))
}

pub fn update_item(conn: &Connection, item: &Item) -> Result<()> {
    let tags_json = serde_json::to_string(&item.tags)?;
    conn.execute(
        "UPDATE items SET title=?2, url=?3, project=?4, tags_json=?5, path=?6, summary=?7, body=?8, modified=?9
         WHERE uuid=?1",
        rusqlite::params![
            item.uuid.to_string(),
            item.title,
            item.url,
            item.project,
            tags_json,
            item.path,
            item.summary,
            item.body,
            dt_to_str(&item.modified),
        ],
    )?;
    set_item_tags(conn, &item.uuid, &item.tags)?;
    Ok(())
}

pub fn archive_item(conn: &Connection, uuid: &Uuid) -> Result<()> {
    conn.execute(
        "UPDATE items SET status='archived', modified=?2 WHERE uuid=?1",
        rusqlite::params![uuid.to_string(), dt_to_str(&Utc::now())],
    )?;
    Ok(())
}

#[cfg(test)]
pub fn item_status_for_test(conn: &Connection, uuid: &str) -> String {
    conn.query_row("SELECT status FROM items WHERE uuid = ?1", [uuid], |r| {
        r.get(0)
    })
    .unwrap()
}

pub fn promote_item(conn: &Connection, uuid: &Uuid) -> Result<bool> {
    let n = conn.execute(
        "UPDATE items SET status='active', modified=?2 WHERE uuid=?1 AND status='provisional'",
        rusqlite::params![uuid.to_string(), dt_to_str(&Utc::now())],
    )?;
    Ok(n > 0)
}

pub fn set_item_tags(conn: &Connection, item_uuid: &Uuid, tags: &[String]) -> Result<()> {
    let uuid = item_uuid.to_string();
    atomically(conn, "set_item_tags", || {
        conn.execute("DELETE FROM item_tags WHERE item_uuid = ?1", [&uuid])?;
        for tag in tags {
            let folded = fold_tag(tag);
            if folded.is_empty() {
                continue;
            }
            conn.execute(
                "INSERT OR IGNORE INTO item_tags (item_uuid, tag) VALUES (?1, ?2)",
                rusqlite::params![uuid, folded],
            )?;
        }
        Ok(())
    })
}

pub fn set_item_projects(conn: &Connection, item_uuid: &Uuid, projects: &[String]) -> Result<()> {
    let uuid = item_uuid.to_string();
    atomically(conn, "set_item_projects", || {
        conn.execute("DELETE FROM item_projects WHERE item_uuid = ?1", [&uuid])?;
        for project in projects {
            let trimmed = project.trim();
            if trimmed.is_empty() {
                continue;
            }
            conn.execute(
                "INSERT OR IGNORE INTO item_projects (item_uuid, project) VALUES (?1, ?2)",
                rusqlite::params![uuid, trimmed],
            )?;
        }
        Ok(())
    })
}

pub fn get_item_projects(conn: &Connection, item_uuid: &Uuid) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT project FROM item_projects WHERE item_uuid = ?1 ORDER BY project")?;
    let rows = stmt.query_map([item_uuid.to_string()], |r| r.get::<_, String>(0))?;
    let mut out = vec![];
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn all_item_projects(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT item_uuid, project FROM item_projects")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = vec![];
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn set_item_files(conn: &Connection, item_uuid: &Uuid, paths: &[String]) -> Result<()> {
    let uuid = item_uuid.to_string();
    atomically(conn, "set_item_files", || {
        conn.execute("DELETE FROM item_files WHERE item_uuid = ?1", [&uuid])?;
        for path in paths {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                continue;
            }
            conn.execute(
                "INSERT OR IGNORE INTO item_files (item_uuid, file_path) VALUES (?1, ?2)",
                rusqlite::params![uuid, trimmed],
            )?;
        }
        Ok(())
    })
}

pub fn get_item_files(conn: &Connection, item_uuid: &Uuid) -> Result<Vec<String>> {
    let uuid = item_uuid.to_string();
    let mut stmt =
        conn.prepare("SELECT file_path FROM item_files WHERE item_uuid = ?1 ORDER BY file_path")?;
    let rows = stmt.query_map([&uuid], |r| r.get(0))?;
    let mut paths = vec![];
    for r in rows {
        paths.push(r?);
    }
    Ok(paths)
}

pub fn find_items_by_file(conn: &Connection, path: &str, prefix: bool) -> Result<Vec<Item>> {
    let mut items = vec![];
    if prefix {
        let mut stmt = conn.prepare(
            "SELECT i.uuid, i.kind, i.display_id, i.title, i.url, i.project, i.tags_json, i.path, i.summary, i.body, i.created, i.modified, i.status, i.source_task_uuid
             FROM items i JOIN item_files f ON f.item_uuid = i.uuid
             WHERE i.status IN ('active','provisional') AND f.file_path LIKE ?1 ESCAPE '\\'
             ORDER BY i.modified DESC",
        )?;
        let rows = stmt.query_map([like_prefix_pattern(path)], row_to_item)?;
        for r in rows {
            items.push(r?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT i.uuid, i.kind, i.display_id, i.title, i.url, i.project, i.tags_json, i.path, i.summary, i.body, i.created, i.modified, i.status, i.source_task_uuid
             FROM items i JOIN item_files f ON f.item_uuid = i.uuid
             WHERE i.status IN ('active','provisional') AND f.file_path = ?1
             ORDER BY i.modified DESC",
        )?;
        let rows = stmt.query_map([path], row_to_item)?;
        for r in rows {
            items.push(r?);
        }
    }
    Ok(items)
}

pub fn set_item_task_links(
    conn: &Connection,
    item_uuid: &Uuid,
    links: &[(Uuid, &str)],
) -> Result<()> {
    let uuid = item_uuid.to_string();
    atomically(conn, "set_item_task_links", || {
        conn.execute("DELETE FROM item_task_links WHERE item_uuid = ?1", [&uuid])?;
        for (task_uuid, source) in links {
            conn.execute(
            "INSERT OR IGNORE INTO item_task_links (item_uuid, task_uuid, source) VALUES (?1, ?2, ?3)",
            rusqlite::params![uuid, task_uuid.to_string(), source],
        )?;
        }
        Ok(())
    })
}

pub fn find_tasks_by_file(conn: &Connection, path: &str, prefix: bool) -> Result<Vec<Task>> {
    let mut tasks = vec![];
    if prefix {
        let mut stmt = conn.prepare(&format!(
            "SELECT DISTINCT t.{TASK_COLUMNS} FROM tasks t
             JOIN task_files f ON f.task_uuid = t.uuid
             WHERE t.status = 'completed' AND f.path LIKE ?1 ESCAPE '\\'
             ORDER BY t.modified DESC"
        ))?;
        let rows = stmt.query_map([like_prefix_pattern(path)], row_to_task)?;
        for r in rows {
            tasks.push(r?);
        }
    } else {
        let mut stmt = conn.prepare(&format!(
            "SELECT DISTINCT t.{TASK_COLUMNS} FROM tasks t
             JOIN task_files f ON f.task_uuid = t.uuid
             WHERE t.status = 'completed' AND f.path = ?1
             ORDER BY t.modified DESC"
        ))?;
        let rows = stmt.query_map([path], row_to_task)?;
        for r in rows {
            tasks.push(r?);
        }
    }
    Ok(tasks)
}

pub fn get_item_task_links(conn: &Connection, item_uuid: &Uuid) -> Result<Vec<(Task, String)>> {
    let uuid = item_uuid.to_string();
    let mut stmt = conn.prepare(&format!(
        "SELECT t.{TASK_COLUMNS}, l.source
         FROM item_task_links l
         JOIN tasks t ON t.uuid = l.task_uuid
         WHERE l.item_uuid = ?1
         ORDER BY t.modified DESC"
    ))?;
    let rows = stmt.query_map([&uuid], |row| {
        let task = row_to_task(row)?;
        let source: String = row.get(16)?;
        Ok((task, source))
    })?;
    let mut result = vec![];
    for r in rows {
        result.push(r?);
    }
    Ok(result)
}

pub fn has_any_memories(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM items WHERE kind = 'memory' AND status IN ('active','provisional')",
        [],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

pub fn list_tags_with_counts(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT it.tag, COUNT(*) FROM item_tags it
         JOIN items i ON i.uuid = it.item_uuid
         WHERE i.status IN ('active','provisional')
         GROUP BY it.tag ORDER BY COUNT(*) DESC, it.tag ASC",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn find_items_by_tag(conn: &Connection, tag: &str) -> Result<Vec<Item>> {
    let folded = fold_tag(tag);
    let mut stmt = conn.prepare(
        "SELECT i.uuid, i.kind, i.display_id, i.title, i.url, i.project, i.tags_json, i.path, i.summary, i.body, i.created, i.modified, i.status, i.source_task_uuid
         FROM items i JOIN item_tags it ON it.item_uuid = i.uuid
         WHERE i.status IN ('active','provisional') AND it.tag = ?1 ORDER BY i.modified DESC",
    )?;
    let rows = stmt.query_map([folded], row_to_item)?;
    let mut items = vec![];
    for r in rows {
        items.push(r?);
    }
    Ok(items)
}

pub fn find_items_by_project(conn: &Connection, project: &str) -> Result<Vec<Item>> {
    let mut stmt = conn.prepare(
        "SELECT i.uuid, i.kind, i.display_id, i.title, i.url, i.project, i.tags_json, i.path, i.summary, i.body, i.created, i.modified, i.status, i.source_task_uuid
         FROM items i JOIN item_projects ip ON ip.item_uuid = i.uuid
         WHERE i.status IN ('active','provisional') AND ip.project = ?1 ORDER BY i.modified DESC",
    )?;
    let rows = stmt.query_map([project], row_to_item)?;
    let mut items = vec![];
    for r in rows {
        items.push(r?);
    }
    Ok(items)
}

pub fn find_cross_project_canonicals_for_project(
    conn: &Connection,
    project: &str,
) -> Result<std::collections::HashSet<Uuid>> {
    let mut out = std::collections::HashSet::new();
    for child in find_items_by_project(conn, project)? {
        for link in get_memory_links_from(conn, &child.uuid.to_string())? {
            if link.relation == "derived_from"
                && let Ok(canonical_uuid) = Uuid::parse_str(&link.to_uuid)
            {
                out.insert(canonical_uuid);
            }
        }
    }
    Ok(out)
}
