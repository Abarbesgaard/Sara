use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

use crate::config::Config;
use crate::db;
use crate::tui;
use crate::tui::review_form::{FormContext, FormInput, run_form};

pub fn run(conn: &Connection, cfg: &Config, id_or_uuid: &str) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;

    // Build form context from existing task
    let pending = db::list_tasks(conn, None)?;
    let available_deps: Vec<(String, String)> = pending
        .iter()
        .filter(|t| t.uuid != task.uuid)
        .map(|t| {
            let id = format!("{}", t.id.unwrap_or(0));
            (id, t.description.clone())
        })
        .collect();

    let project_files: Vec<String> = db::get_project(conn, &task.project)?
        .and_then(|p| p.path)
        .map(|p| crate::files::collect_project_entries(std::path::Path::new(&p)))
        .unwrap_or_default();

    let current_files = db::get_task_files(conn, &task.uuid)?;

    let due_str = task
        .due
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default();

    let ctx = FormContext {
        initial: FormInput {
            description: task.description.clone(),
            project: task.project.clone(),
            priority: task.priority.clone(),
            due: due_str,
            tags: task.tags.join(","),
            selected_deps: vec![],
            selected_files: current_files,
        },
        available_deps,
        available_files: project_files,
        suggested_dep_indices: vec![],
        suggested_files: vec![],
    };

    let mut terminal = tui::init_terminal()?;
    let result = run_form(&mut terminal, ctx);
    tui::restore_terminal()?;

    let Some(form) = result? else {
        println!("Cancelled.");
        return Ok(());
    };

    // Apply changes
    let mut updated = task.clone();
    updated.description = form.description;
    updated.project = form.project.clone();
    updated.priority = form.priority;
    updated.tags = form
        .tags
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    updated.modified = Utc::now();

    if form.due.is_empty() {
        updated.due = None;
    } else {
        updated.due = crate::commands::add::parse_due(&form.due, cfg);
    }

    updated.urgency = db::compute_urgency(&updated, &cfg.urgency, false, 0);
    db::update_task(conn, &updated)?;

    // Update files (paths come directly from the form).
    db::set_task_files(conn, &updated.uuid, &form.selected_files)?;

    db::refresh_urgency(conn, &cfg.urgency, &updated.uuid)?;

    println!(
        "Updated task {}: {}",
        updated.id.unwrap_or(0),
        updated.description
    );
    Ok(())
}
