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
    annotations: &[String],
    links: &[String],
    checks: &[String],
    depends_on: &[String],
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

    for prefix in depends_on {
        match db::get_task_by_uuid_prefix(conn, prefix) {
            Ok(Some(dep)) => {
                if let Err(e) = db::add_dependency(conn, &task.uuid, &dep.uuid) {
                    eprintln!("Warning: could not add dependency on {prefix}: {e}");
                } else {
                    db::refresh_urgency(conn, &cfg.urgency, &dep.uuid).ok();
                }
            }
            Ok(None) => {
                eprintln!("Warning: no task found for prefix '{prefix}', skipping dependency")
            }
            Err(e) => eprintln!("Warning: could not resolve '{prefix}': {e}"),
        }
    }

    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;

    for text in annotations {
        if let Err(e) =
            db::add_annotation_full(conn, &task.uuid, text, "comment", "ai", None, None, false)
        {
            eprintln!("Warning: could not add annotation: {e}");
        }
    }
    for url in links {
        if let Err(e) = db::add_link(conn, &task.uuid, url, None) {
            eprintln!("Warning: could not add link: {e}");
        }
    }
    for text in checks {
        if let Err(e) = db::add_step(conn, &task.uuid, text, None, "step", "human", None) {
            eprintln!("Warning: could not add step: {e}");
        }
    }

    println!(
        "Created task {} [{}] ({}): {}",
        task.id.unwrap_or(0),
        task.project,
        &task.uuid.to_string()[..8],
        task.description
    );
    Ok(())
}
