use anyhow::{Context, Result};
use rusqlite::Connection;
use uuid::Uuid;

use crate::commands::shared::summarize;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::memory::embedding;
use crate::infrastructure::model::Item;
use crate::infrastructure::project::detect_current_project;

pub(super) fn save(
    conn: &Connection,
    cfg: &Config,
    text: &str,
    tags: &[String],
    projects: &[String],
    task_prefixes: &[String],
    files: &[String],
) -> Result<Item> {
    if text.is_empty() {
        anyhow::bail!("Memory text cannot be empty");
    }

    let first_task_uuid: Option<Uuid> = match task_prefixes.first() {
        Some(prefix) => Some(
            db::resolve_task(conn, prefix)
                .with_context(|| format!("looking up --task '{prefix}'"))?
                .uuid,
        ),
        None => None,
    };

    let projects: Vec<String> = if projects.is_empty() {
        let (name, _) = detect_current_project(conn, cfg)?;
        vec![name]
    } else {
        projects.to_vec()
    };

    let mut item = Item::new_memory(summarize(text), text.to_string(), first_task_uuid);
    item.tags = tags.to_vec();
    item.path = Some(String::new());

    db::insert_item(conn, &mut item)?;
    db::set_item_projects(conn, &item.uuid, &projects)?;

    embedding::index_memory(conn, &item);

    if !files.is_empty() {
        db::set_item_files(conn, &item.uuid, files)?;
    }

    let mut task_links: Vec<(Uuid, &'static str)> = vec![];

    for file in files {
        let found = db::find_tasks_by_file(conn, file, false)?;
        for t in found {
            if !task_links.iter().any(|(u, _)| *u == t.uuid) {
                task_links.push((t.uuid, "auto"));
            }
        }
    }

    for prefix in task_prefixes {
        if let Ok(t) = db::resolve_task(conn, prefix) {
            if let Some(pos) = task_links.iter().position(|(u, _)| *u == t.uuid) {
                task_links[pos].1 = "explicit";
            } else {
                task_links.push((t.uuid, "explicit"));
            }
        }
    }

    if !task_links.is_empty() {
        let owned: Vec<(Uuid, &str)> = task_links.iter().map(|(u, s)| (*u, *s)).collect();
        db::set_item_task_links(conn, &item.uuid, &owned)?;

        let links = db::get_item_task_links(conn, &item.uuid)?;
        item.linked_tasks = links
            .into_iter()
            .map(|(t, src)| {
                (
                    t.id.map(|i| i.to_string()).unwrap_or_default(),
                    t.description,
                    src,
                )
            })
            .collect();
    }

    Ok(item)
}
