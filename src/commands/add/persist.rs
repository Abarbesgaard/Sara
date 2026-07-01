use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Task;
use crate::infrastructure::tui::review_form::FormInput;

pub(super) fn save(
    conn: &Connection,
    cfg: &Config,
    form: FormInput,
    recur: Option<String>,
) -> Result<()> {
    let mut task = Task::new(form.description, form.project.clone());
    task.priority = form.priority;
    task.tags = form
        .tags
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    task.recur = recur;

    if !form.due.is_empty() {
        task.due = super::parse_due(&form.due, cfg);
    }

    task.urgency = db::compute_urgency(&task, &cfg.urgency, false, 0);

    db::insert_task(conn, &mut task)?;

    if !form.selected_files.is_empty() {
        db::set_task_files(conn, &task.uuid, &form.selected_files)?;
    }

    let pending = db::list_tasks(conn, None)?;
    for &dep_idx in &form.selected_deps {
        if let Some(dep_task) = pending.get(dep_idx)
            && let Err(e) = db::add_dependency(conn, &task.uuid, &dep_task.uuid)
        {
            eprintln!("Warning: could not add dependency: {e}");
        }
    }

    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;

    println!(
        "Created task {} [{}] ({}): {}",
        task.id.unwrap_or(0),
        task.project,
        &task.uuid.to_string()[..8],
        task.description
    );
    Ok(())
}
