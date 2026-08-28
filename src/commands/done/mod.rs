use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Status;

/// Complete a task and return a structured record of what happened (including any
/// spawned recurrence). Print-free core shared by the CLI `done` command and the
/// MCP `done` tool. Errors if the task is blocked and `force` is false.
pub fn done_value(conn: &Connection, cfg: &Config, id_or_uuid: &str, force: bool) -> Result<Value> {
    let mut task = db::resolve_task(conn, id_or_uuid)?;
    crate::commands::guide::guard_branch_mutation(conn, id_or_uuid, &task, force)?;

    // Check blockers
    let blockers = db::get_blockers(conn, &task.uuid)?;
    if !blockers.is_empty() && !force {
        let blocker_ids: Vec<String> = blockers
            .iter()
            .map(|u| {
                db::get_task_by_uuid_prefix(conn, &u.to_string()[..8])
                    .ok()
                    .flatten()
                    .and_then(|t| t.id)
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| u.to_string()[..8].to_string())
            })
            .collect();
        anyhow::bail!(
            "Task {} is blocked by tasks: {}. Use --force to complete anyway.",
            task.id.unwrap_or(0),
            blocker_ids.join(", ")
        );
    }

    // Finalize any running timer
    if let Some(started) = task.started_at {
        task.time_spent += (Utc::now() - started).num_seconds().max(0);
        task.started_at = None;
    }

    task.status = Status::Completed;
    task.end = Some(Utc::now());
    task.modified = Utc::now();
    db::update_task(conn, &task)?;

    // Repack display IDs
    db::repack_ids(conn)?;

    // Refresh urgency for tasks that were blocking on this one
    let was_blocking = db::get_blocking(conn, &task.uuid)?;
    for dep_uuid in was_blocking {
        let _ = db::refresh_urgency(conn, &cfg.urgency, &dep_uuid);
    }

    // Spawn next occurrence for recurring tasks
    let mut recurrence = Value::Null;
    if let Some(ref interval) = task.recur.clone() {
        let base = task.due.unwrap_or_else(Utc::now);
        let next_due = crate::infrastructure::model::advance_by_interval(base, interval);
        let mut next =
            crate::infrastructure::model::Task::new(task.description.clone(), task.project.clone());
        next.priority = task.priority.clone();
        next.tags = task.tags.clone();
        next.due = Some(next_due);
        next.recur = Some(interval.clone());
        next.estimate_mins = task.estimate_mins;
        next.urgency = db::compute_urgency(&next, &cfg.urgency, false, 0);
        db::insert_task(conn, &mut next)?;
        recurrence = json!({
            "id": next.id,
            "due": next_due.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string(),
        });
    }

    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "project": task.project,
        "description": task.description,
        "status": "completed",
        "recurrence": recurrence,
        "auto_memory": db::synthesize_done_memory(conn, &task.uuid, &task.project)
            .unwrap_or(None),
        "hygiene": db::hygiene_pass(conn).map(|h| json!({
            "archived": h.archived,
            "review_pending": h.review_pending,
            "oldest_age_days": h.oldest_age_days,
        })).unwrap_or(Value::Null),
    }))
}

pub fn run(conn: &Connection, cfg: &Config, id_or_uuid: &str, force: bool) -> Result<()> {
    let v = done_value(conn, cfg, id_or_uuid, force)?;
    println!(
        "Done: [{}] {}",
        v["project"].as_str().unwrap_or_default(),
        v["description"].as_str().unwrap_or_default()
    );
    if let Some(rec) = v.get("recurrence").filter(|r| !r.is_null()) {
        println!(
            "♺  Next recurrence: #{} due {}",
            rec.get("id").and_then(|i| i.as_i64()).unwrap_or(0),
            rec.get("due").and_then(|d| d.as_str()).unwrap_or_default()
        );
    }
    if let Some(label) = v.get("auto_memory").and_then(|m| m.as_str()) {
        println!("🧠 Auto-memory saved: {label} (provisional — review with `sara memories`)");
    }
    if let Some(hyg) = v.get("hygiene").filter(|h| !h.is_null()) {
        let archived: Vec<&str> = hyg
            .get("archived")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();
        if !archived.is_empty() {
            println!(
                "🧹 Auto-archived {} superseded {}: {}",
                archived.len(),
                if archived.len() == 1 {
                    "memory"
                } else {
                    "memories"
                },
                archived.join(", ")
            );
        }
        let pending = hyg
            .get("review_pending")
            .and_then(|n| n.as_u64())
            .unwrap_or(0);
        if pending > 0 {
            let oldest = hyg
                .get("oldest_age_days")
                .and_then(|d| d.as_i64())
                .unwrap_or(0);
            println!(
                "🧹 {} {} await review (oldest {}d) — run `sara prune-memories` to triage",
                pending,
                if pending == 1 { "memory" } else { "memories" },
                oldest
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod hygiene_tests {
    use super::*;
    use crate::infrastructure::model::{Item, Task};

    /// Create an active memory via db primitives (no cross-slice learn import).
    fn mem(conn: &Connection, title: &str) -> Item {
        let mut item = Item::new_memory(title.to_string(), format!("body of {title}"), None);
        item.path = Some(String::new());
        db::insert_item(conn, &mut item).unwrap();
        item
    }

    fn label(item: &Item) -> String {
        format!("m{}", item.display_id.unwrap_or(0))
    }

    #[test]
    fn done_auto_archives_superseded() {
        let conn = db::open_in_memory_for_test();
        let old = mem(&conn, "old finding");
        let new = mem(&conn, "new finding replaces old");
        // new supersedes old (new is active).
        db::insert_memory_link(
            &conn,
            &new.uuid.to_string(),
            &old.uuid.to_string(),
            "supersedes",
            1.0,
        )
        .unwrap();

        let mut task = Task::new("hygiene demo".into(), "proj".into());
        db::insert_task(&conn, &mut task).unwrap();
        let v = done_value(&conn, &Config::default(), &task.uuid.to_string(), false).unwrap();

        let archived: Vec<&str> = v["hygiene"]["archived"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|x| x.as_str())
            .collect();
        assert!(
            archived.contains(&label(&old).as_str()),
            "superseded old memory is auto-archived"
        );

        // Archived memories drop out of the active read path; the superseder stays.
        assert!(
            db::get_item_by_handle(&conn, &label(&old)).is_err(),
            "old memory is no longer active"
        );
        assert!(
            db::get_item_by_handle(&conn, &label(&new)).is_ok(),
            "superseding memory stays active"
        );
    }
}
