use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

use crate::config::Config;
use crate::db;
use crate::model::Task;
use crate::tui;
use crate::tui::review_form::{FormContext, FormInput, run_form};

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
) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;

    // Non-interactive mode: if any field flag is present, apply it directly via
    // db::update_task and skip the review-form TUI entirely.
    let has_field_flags = description.is_some()
        || priority.is_some()
        || due.is_some()
        || clear_due
        || !tags.is_empty()
        || clear_tags;
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
        );
    }

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

/// Apply field changes non-interactively (no TUI), reusing `db::update_task`.
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
) -> Result<()> {
    let mut updated = merge_task_fields(
        task,
        cfg,
        description,
        priority,
        due,
        clear_due,
        tags,
        clear_tags,
    )?;

    updated.urgency = db::compute_urgency(&updated, &cfg.urgency, false, 0);
    db::update_task(conn, &updated)?;
    db::refresh_urgency(conn, &cfg.urgency, &updated.uuid)?;

    println!(
        "Updated task {}: {}",
        updated.id.unwrap_or(0),
        updated.description
    );
    Ok(())
}

/// Pure field-merge: apply the CLI setter flags onto a `Task`. No DB / IO, so it
/// is unit-testable. Returns an error on an unparseable priority or due date.
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

    // `--clear-tags` wins; otherwise `--tag` (repeatable) replaces the tag set.
    if clear_tags {
        updated.tags = vec![];
    } else if !tags.is_empty() {
        updated.tags = tags
            .iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
    }

    if clear_due {
        updated.due = None;
    } else if let Some(d) = due {
        match crate::commands::add::parse_due(d, cfg) {
            Some(dt) => updated.due = Some(dt),
            None => anyhow::bail!("Could not parse due date: {d}"),
        }
    }

    updated.modified = Utc::now();
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Priority, Status, Task};
    use uuid::Uuid;

    fn sample() -> Task {
        Task {
            uuid: Uuid::new_v4(),
            id: Some(1),
            description: "orig".into(),
            project: "p".into(),
            status: Status::Pending,
            priority: None,
            due: None,
            entry: Utc::now(),
            modified: Utc::now(),
            end: None,
            tags: vec!["old".into()],
            urgency: 0.0,
            started_at: None,
            time_spent: 0,
            estimate_mins: None,
            recur: None,
        }
    }

    #[test]
    fn sets_description_priority_and_replaces_tags() {
        let cfg = Config::default();
        let t = merge_task_fields(
            sample(),
            &cfg,
            Some("new desc"),
            Some("h"),
            None,
            false,
            &["a".into(), "b".into()],
            false,
        )
        .unwrap();
        assert_eq!(t.description, "new desc");
        assert_eq!(t.priority, Some(Priority::H));
        assert_eq!(t.tags, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn clear_tags_and_clear_due_unset_fields() {
        let cfg = Config::default();
        let mut base = sample();
        base.due = Some(Utc::now());
        let t = merge_task_fields(base, &cfg, None, None, None, true, &[], true).unwrap();
        assert!(t.tags.is_empty());
        assert!(t.due.is_none());
    }

    #[test]
    fn invalid_priority_is_rejected() {
        let cfg = Config::default();
        assert!(
            merge_task_fields(sample(), &cfg, None, Some("X"), None, false, &[], false).is_err()
        );
    }

    #[test]
    fn invalid_due_is_rejected() {
        let cfg = Config::default();
        assert!(
            merge_task_fields(
                sample(),
                &cfg,
                None,
                None,
                Some("not-a-date"),
                false,
                &[],
                false
            )
            .is_err()
        );
    }

    #[test]
    fn unspecified_fields_are_left_unchanged() {
        let cfg = Config::default();
        let t = merge_task_fields(sample(), &cfg, None, None, None, false, &[], false).unwrap();
        assert_eq!(t.description, "orig");
        assert_eq!(t.priority, None);
        assert_eq!(t.tags, vec!["old".to_string()]);
    }
}
