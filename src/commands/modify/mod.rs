use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::commands::shared::{
    normalize_list, parse_due, parse_duration_mins, print_cancelled, split_csv,
};
use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Task;
use crate::infrastructure::tui;
use crate::infrastructure::tui::review_form::{FormContext, FormInput, run_form};

mod render;

#[allow(clippy::too_many_arguments)]
pub fn run(
    conn: &Connection,
    cfg: &Config,
    id_or_uuid: &str,
    description: Option<&str>,
    priority: Option<&str>,
    due: Option<&str>,
    clear_due: bool,
    tags: &[String],
    clear_tags: bool,
    estimate: Option<&str>,
    clear_estimate: bool,
    every: Option<&str>,
    clear_recur: bool,
) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;

    let has_field_flags = description.is_some()
        || priority.is_some()
        || due.is_some()
        || clear_due
        || !tags.is_empty()
        || clear_tags
        || estimate.is_some()
        || clear_estimate
        || every.is_some()
        || clear_recur;
    if has_field_flags {
        return apply_fields(
            conn,
            cfg,
            task,
            description,
            priority,
            due,
            clear_due,
            tags,
            clear_tags,
            estimate,
            clear_estimate,
            every,
            clear_recur,
        );
    }

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
        .map(|p| {
            crate::infrastructure::util::files::collect_project_entries(std::path::Path::new(&p))
        })
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

    let result = tui::with_terminal(|t| run_form(t, ctx));

    let Some(form) = result? else {
        print_cancelled();
        return Ok(());
    };

    let mut updated = task.clone();
    updated.description = form.description;
    updated.project = form.project.clone();
    updated.priority = form.priority;
    updated.tags = split_csv(&form.tags);
    updated.modified = Utc::now();

    if form.due.is_empty() {
        updated.due = None;
    } else {
        updated.due = parse_due(&form.due, cfg);
    }

    updated.urgency = db::compute_urgency(&updated, &cfg.urgency, false, 0);
    db::update_task(conn, &updated)?;

    db::set_task_files(conn, &updated.uuid, &form.selected_files)?;

    db::refresh_urgency(conn, &cfg.urgency, &updated.uuid)?;

    render::print_updated(updated.id.unwrap_or(0), &updated.description);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn modify_value(
    conn: &Connection,
    cfg: &Config,
    id_or_uuid: &str,
    description: Option<&str>,
    priority: Option<&str>,
    due: Option<&str>,
    clear_due: bool,
    tags: &[String],
    clear_tags: bool,
    estimate: Option<&str>,
    clear_estimate: bool,
    every: Option<&str>,
    clear_recur: bool,
) -> Result<Value> {
    let has_field = description.is_some()
        || priority.is_some()
        || due.is_some()
        || clear_due
        || !tags.is_empty()
        || clear_tags
        || estimate.is_some()
        || clear_estimate
        || every.is_some()
        || clear_recur;
    anyhow::ensure!(
        has_field,
        "modify requires at least one field to change (description, priority, due, clear_due, tags, clear_tags, estimate, clear_estimate, every, or clear_recur)"
    );

    let task = db::resolve_task(conn, id_or_uuid)?;
    let mut updated = merge_task_fields(
        task,
        cfg,
        description,
        priority,
        due,
        clear_due,
        tags,
        clear_tags,
        estimate,
        clear_estimate,
        every,
        clear_recur,
    )?;

    updated.urgency = db::compute_urgency(&updated, &cfg.urgency, false, 0);
    db::update_task(conn, &updated)?;
    db::refresh_urgency(conn, &cfg.urgency, &updated.uuid)?;

    Ok(json!({
        "task": updated.id,
        "uuid": updated.uuid.to_string(),
        "description": updated.description,
        "priority": updated.priority.as_ref().map(|p| p.label()),
        "due": updated.due.map(|d| d.format("%Y-%m-%d").to_string()),
        "tags": updated.tags,
        "estimate_mins": updated.estimate_mins,
        "recur": updated.recur,
    }))
}

#[allow(clippy::too_many_arguments)]
fn apply_fields(
    conn: &Connection,
    cfg: &Config,
    task: Task,
    description: Option<&str>,
    priority: Option<&str>,
    due: Option<&str>,
    clear_due: bool,
    tags: &[String],
    clear_tags: bool,
    estimate: Option<&str>,
    clear_estimate: bool,
    every: Option<&str>,
    clear_recur: bool,
) -> Result<()> {
    let uuid = task.uuid.to_string();
    let v = modify_value(
        conn,
        cfg,
        &uuid,
        description,
        priority,
        due,
        clear_due,
        tags,
        clear_tags,
        estimate,
        clear_estimate,
        every,
        clear_recur,
    )?;
    render::print_updated(
        v["task"].as_i64().unwrap_or(0),
        v["description"].as_str().unwrap_or_default(),
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn merge_task_fields(
    task: Task,
    cfg: &Config,
    description: Option<&str>,
    priority: Option<&str>,
    due: Option<&str>,
    clear_due: bool,
    tags: &[String],
    clear_tags: bool,
    estimate: Option<&str>,
    clear_estimate: bool,
    every: Option<&str>,
    clear_recur: bool,
) -> Result<Task> {
    let mut updated = task;

    if let Some(d) = description {
        updated.description = d.to_string();
    }

    if let Some(p) = priority {
        updated.priority = Some(
            p.parse()
                .map_err(|_| anyhow::anyhow!("Unknown priority: {p} (expected H, M, or L)"))?,
        );
    }

    if clear_tags {
        updated.tags = vec![];
    } else if !tags.is_empty() {
        updated.tags = normalize_list(tags);
    }

    if clear_due {
        updated.due = None;
    } else if let Some(d) = due {
        match parse_due(d, cfg) {
            Some(dt) => updated.due = Some(dt),
            None => anyhow::bail!("Could not parse due date: {d}"),
        }
    }

    if clear_estimate {
        updated.estimate_mins = None;
    } else if let Some(e) = estimate {
        match parse_duration_mins(e) {
            Some(mins) => updated.estimate_mins = Some(mins),
            None => anyhow::bail!(
                "Could not parse estimate: {e} (expected e.g. \"90m\", \"2h\", \"2h30m\")"
            ),
        }
    }

    if clear_recur {
        updated.recur = None;
    } else if let Some(r) = every {
        updated.recur = if r.trim().is_empty() {
            None
        } else {
            Some(r.trim().to_string())
        };
    }

    updated.modified = Utc::now();
    Ok(updated)
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/modify/mod.rs"]
mod tests;
