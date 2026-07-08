use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Item;
use crate::infrastructure::project::detect_current_project;

/// `sara learn "<text>" [--tag] [-p] [--task]` — save a short, freeform
/// distillation as a global memory (not task/project-scoped: it *references*
/// projects rather than belonging to one, same convention as `sara add`).
pub fn run(
    conn: &Connection,
    cfg: &Config,
    text: &str,
    tags: &[String],
    projects: &[String],
    task: Option<&str>,
) -> Result<()> {
    let item = save(conn, cfg, text, tags, projects, task)?;

    println!(
        "Learned m{} ({}): {}",
        item.display_id.unwrap_or(0),
        &item.uuid.to_string()[..8],
        summarize(text)
    );
    Ok(())
}

fn save(
    conn: &Connection,
    cfg: &Config,
    text: &str,
    tags: &[String],
    projects: &[String],
    task: Option<&str>,
) -> Result<Item> {
    let text = text.trim();
    if text.is_empty() {
        anyhow::bail!("Memory text cannot be empty");
    }

    let source_task_uuid = match task {
        Some(prefix) => Some(
            db::get_task_by_uuid_prefix(conn, prefix)
                .context("looking up --task")?
                .ok_or_else(|| anyhow::anyhow!("No task found for prefix '{prefix}'"))?
                .uuid,
        ),
        None => None,
    };

    let projects: Vec<String> = if projects.is_empty() {
        let (name, _path) = detect_current_project(conn, cfg)?;
        vec![name]
    } else {
        projects.to_vec()
    };

    let mut item = Item::new_memory(summarize(text), text.to_string(), source_task_uuid);
    item.tags = tags.to_vec();
    item.path = Some(String::new());

    db::insert_item(conn, &mut item)?;
    db::set_item_projects(conn, &item.uuid, &projects)?;

    Ok(item)
}

/// A short title for display, taken from the start of the memory text.
fn summarize(text: &str) -> String {
    const MAX: usize = 80;
    let trimmed = text.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        let truncated: String = trimmed.chars().take(MAX).collect();
        format!("{}…", truncated.trim_end())
    }
}
