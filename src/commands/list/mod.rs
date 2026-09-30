use anyhow::Result;
use rusqlite::Connection;

use crate::commands::shared::print_json;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Task;
use crate::infrastructure::project::detect_current_project;

mod render;

fn resolve_filter(
    conn: &Connection,
    cfg: &Config,
    all: bool,
    project_filter: Option<&str>,
) -> Result<Option<String>> {
    if all {
        Ok(None)
    } else if let Some(p) = project_filter {
        Ok(Some(p.to_string()))
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
                "id": t.id,
                "uuid": t.uuid.to_string(),
                "description": t.description,
                "project": t.project,
                "priority": t.priority.as_ref().map(|p| p.label()),
                "due": t.due.map(|d| d.to_rfc3339()),
                "urgency": t.urgency,
                "status": t.status.to_string(),
                "tags": t.tags,
            })
        })
        .collect();
    serde_json::json!({ "tasks": arr })
}

pub fn list_value(
    conn: &Connection,
    cfg: &Config,
    all: bool,
    project_filter: Option<&str>,
) -> Result<serde_json::Value> {
    let filter = resolve_filter(conn, cfg, all, project_filter)?;
    let tasks = db::list_tasks(conn, filter.as_deref())?;
    Ok(tasks_to_value(&tasks))
}

pub fn run(
    conn: &Connection,
    cfg: &Config,
    all: bool,
    project_filter: Option<&str>,
    as_json: bool,
    by_issue: bool,
) -> Result<()> {
    let no_color = std::env::var("NO_COLOR").is_ok();

    let filter = resolve_filter(conn, cfg, all, project_filter)?;

    let tasks = db::list_tasks(conn, filter.as_deref())?;
    let link_flags = db::link_flags_by_task(conn).unwrap_or_default();
    let github_synced = db::github_synced_task_uuids(conn).unwrap_or_default();
    let dep_info = db::dep_info_by_task(conn).unwrap_or_default();

    if as_json {
        print_json(&tasks_to_value(&tasks))?;
        return Ok(());
    }

    if by_issue {
        return render::print_by_issue(conn, &tasks, no_color);
    }

    if tasks.is_empty() {
        let scope = filter
            .as_deref()
            .map(|p| format!("project '{p}'"))
            .unwrap_or_else(|| "any project".to_string());
        println!("No pending tasks for {scope}.");
        if !all && filter.is_some() {
            let all_tasks = db::list_tasks(conn, None)?;
            if !all_tasks.is_empty() {
                println!(
                    "Tip: run `sara list -a` to see all {} pending tasks.",
                    all_tasks.len()
                );
            }
        }
        return Ok(());
    }

    render::print_table(
        &tasks,
        &link_flags,
        &github_synced,
        &dep_info,
        filter.as_deref(),
        no_color,
    );

    Ok(())
}
