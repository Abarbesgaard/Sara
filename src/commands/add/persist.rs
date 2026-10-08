use anyhow::Result;
use rusqlite::Connection;

use super::types::AddRequest;
use crate::commands::shared::{parse_due, parse_duration_mins, project_path, split_csv};
use crate::commands::{begin, guide};
use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Task;
use crate::infrastructure::tui::review_form::FormInput;

pub(super) fn save(
    conn: &Connection,
    cfg: &Config,
    form: FormInput,
    recur: Option<String>,
    req: &AddRequest,
) -> Result<Task> {
    let mut task = Task::new(form.description.clone(), form.project.clone());
    task.priority = form.priority.clone();
    task.tags = split_csv(&form.tags);
    task.recur = recur;
    task.estimate_mins = parse_duration_mins(&form.estimate);

    if !form.due.is_empty() {
        task.due = parse_due(&form.due, cfg);
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

    for prefix in req.depends_on {
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

    for text in req.annotations {
        if let Err(e) =
            db::add_annotation_full(conn, &task.uuid, text, "comment", "ai", None, None, false)
        {
            eprintln!("Warning: could not add annotation: {e}");
        }
    }
    for url in req.links.iter().cloned().chain(form.link_list()) {
        if let Err(e) = db::add_link(conn, &task.uuid, &url, None) {
            eprintln!("Warning: could not add link: {e}");
        }
    }
    for text in req.checks {
        if let Err(e) = db::add_step(conn, &task.uuid, text, None, "step", "human", None) {
            eprintln!("Warning: could not add step: {e}");
        }
    }

    if form.has_guide() {
        apply_guide(conn, &task, &form)?;
    }

    Ok(task)
}

fn apply_guide(conn: &Connection, task: &Task, form: &FormInput) -> Result<()> {
    let id = task.id.unwrap_or(0).to_string();
    let assignment = Some(form.assignment.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| form.description.trim());
    guide::assignment_value(conn, &id, assignment)?;
    let why = form.rationale.trim();
    if !why.is_empty() {
        guide::rationale_value(conn, &id, why)?;
    }
    let done_when = form.acceptance.trim();
    if !done_when.is_empty() {
        let verify = Some(form.verify.trim()).filter(|s| !s.is_empty());
        guide::check_value(
            conn,
            &id,
            done_when,
            None,
            Some("acceptance"),
            Some("human"),
            verify,
        )?;
    }
    begin::seed_recall_step(conn, &id)?;
    Ok(())
}

pub(super) fn tie_branch(conn: &Connection, task: &Task) -> Option<String> {
    let path = project_path(conn, &task.project)?;
    let branch = crate::infrastructure::git::current_branch(&path)?;
    db::set_task_branch(conn, &task.uuid, &branch).ok()?;
    Some(branch)
}
