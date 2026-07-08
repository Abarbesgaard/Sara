mod input;
mod persist;
mod similar;

/// How many prior tasks/notes to surface as "similar past work" before creating
/// a new task. Kept small since these are informational, not a hard gate.
const SIMILAR_LIMIT: i64 = 5;

use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::model::Task;

pub fn run(
    conn: &Connection,
    cfg: &Config,
    words: &[String],
    project_override: Option<&str>,
    priority_override: Option<&str>,
    extra_tags: &[String],
    yes: bool,
    recur_override: Option<&str>,
    annotations: &[String],
    links: &[String],
    checks: &[String],
    depends_on: &[String],
) -> Result<()> {
    let Some((form, recur)) = input::resolve(
        conn,
        cfg,
        words,
        project_override,
        priority_override,
        extra_tags,
        yes,
        recur_override,
    )?
    else {
        println!("Cancelled.");
        return Ok(());
    };

    match similar::find_similar(conn, &form.description, SIMILAR_LIMIT) {
        Ok(hits) if !hits.is_empty() => {
            println!("Similar past work found — consider reusing instead of starting fresh:");
            for hit in &hits {
                println!(
                    "  [{}] task {} ({}): {}",
                    hit["ref_kind"].as_str().unwrap_or(""),
                    hit["task"].as_i64().unwrap_or(0),
                    hit["description"].as_str().unwrap_or(""),
                    hit["snippet"].as_str().unwrap_or("")
                );
            }
            println!();
        }
        Ok(_) => {}
        Err(e) => eprintln!("Warning: recall check failed: {e}"),
    }

    let task = persist::save(
        conn,
        cfg,
        form,
        recur,
        annotations,
        links,
        checks,
        depends_on,
    )?;

    println!(
        "Created task {} [{}] ({}): {}",
        task.id.unwrap_or(0),
        task.project,
        &task.uuid.to_string()[..8],
        task.description
    );
    Ok(())
}

/// Create a task and return it — the print-free core for the MCP `add` tool.
/// Always forces `yes = true` so the TUI review form never opens.
#[allow(clippy::too_many_arguments)]
pub fn run_value(
    conn: &Connection,
    cfg: &Config,
    words: &[String],
    project_override: Option<&str>,
    priority_override: Option<&str>,
    extra_tags: &[String],
    recur_override: Option<&str>,
    annotations: &[String],
    links: &[String],
    checks: &[String],
    depends_on: &[String],
) -> Result<serde_json::Value> {
    let Some((form, recur)) = input::resolve(
        conn,
        cfg,
        words,
        project_override,
        priority_override,
        extra_tags,
        true,
        recur_override,
    )?
    else {
        anyhow::bail!("task creation was cancelled");
    };

    let similar = similar::find_similar(conn, &form.description, SIMILAR_LIMIT).unwrap_or_default();

    let task: Task = persist::save(
        conn,
        cfg,
        form,
        recur,
        annotations,
        links,
        checks,
        depends_on,
    )?;

    Ok(serde_json::json!({
        "id": task.id,
        "uuid": task.uuid.to_string(),
        "project": task.project,
        "description": task.description,
        "similar": similar,
    }))
}

pub fn parse_due(s: &str, cfg: &Config) -> Option<chrono::DateTime<chrono::Utc>> {
    crate::infrastructure::dates::parse_due(s, &cfg.date_dialect)
}
