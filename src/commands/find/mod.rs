use anyhow::Result;
use rusqlite::Connection;

use crate::commands::shared::print_json;
use crate::infrastructure::config::Config;
use crate::infrastructure::db::{self, FindStatus};
use crate::infrastructure::model::Task;
use crate::infrastructure::project::detect_current_project;

pub const DEFAULT_LIMIT: usize = 20;

pub fn parse_status(status: Option<&str>) -> Result<FindStatus> {
    match status.map(str::to_ascii_lowercase).as_deref() {
        None | Some("all") => Ok(FindStatus::All),
        Some("pending") => Ok(FindStatus::Pending),
        Some("completed") => Ok(FindStatus::Completed),
        Some(other) => Err(anyhow::anyhow!(
            "unknown status '{other}' — use pending, completed or all"
        )),
    }
}

fn resolve_project(conn: &Connection, cfg: &Config, all: bool) -> Result<Option<String>> {
    if all {
        Ok(None)
    } else {
        let (name, _) = detect_current_project(conn, cfg)?;
        Ok(Some(name))
    }
}

fn tasks_to_value(tasks: &[Task]) -> serde_json::Value {
    let arr: Vec<_> = tasks
        .iter()
        .map(|t| {
            serde_json::json!({
                "uuid": t.uuid.to_string(),
                "id": t.id,
                "project": t.project,
                "status": t.status.to_string(),
                "description": t.description,
            })
        })
        .collect();
    serde_json::json!({ "matches": arr })
}

pub fn find_value(
    conn: &Connection,
    cfg: &Config,
    fragment: &str,
    status: Option<&str>,
    all: bool,
    limit: usize,
) -> Result<serde_json::Value> {
    let status = parse_status(status)?;
    let project = resolve_project(conn, cfg, all)?;
    let tasks = db::find_tasks_by_uuid_fragment(conn, fragment, project.as_deref(), status, limit)?;
    Ok(tasks_to_value(&tasks))
}

pub fn run(
    conn: &Connection,
    cfg: &Config,
    fragment: &str,
    status: Option<&str>,
    all: bool,
    limit: usize,
    as_json: bool,
) -> Result<()> {
    let status_filter = parse_status(status)?;
    let project = resolve_project(conn, cfg, all)?;
    let tasks =
        db::find_tasks_by_uuid_fragment(conn, fragment, project.as_deref(), status_filter, limit)?;

    if as_json {
        print_json(&tasks_to_value(&tasks))?;
        return Ok(());
    }

    if tasks.is_empty() {
        let scope = project
            .as_deref()
            .map(|p| format!("project '{p}'"))
            .unwrap_or_else(|| "any project".to_string());
        println!("No task uuid contains '{fragment}' in {scope}.");
        return Ok(());
    }

    for t in &tasks {
        let short: String = t.uuid.to_string().chars().take(8).collect();
        let id =
            t.id.map(|n| n.to_string())
                .unwrap_or_else(|| "-".to_string());
        println!(
            "{short}  #{id:<4} [{}] {}  ({})",
            t.status, t.description, t.project
        );
    }

    Ok(())
}
