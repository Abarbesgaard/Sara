use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::commands::shared::{
    guard_branch_mutation, json_strs, print_cited, project_head, with_citation,
};
use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::{Status, Task};
use crate::infrastructure::telemetry::{self, Source};

mod types;
use types::DoneGate;

fn done_gate(conn: &Connection, task: &Task) -> Result<DoneGate> {
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;
    if acceptance.is_empty() {
        return Ok(DoneGate::Warn(
            "no acceptance criteria — closed without a proven definition of done".to_string(),
        ));
    }

    let validated = db::get_guide_fields(conn, &task.uuid)?.validated_commit;
    let head = project_head(conn, &task.project);

    match (validated, head) {
        (None, _) => Ok(DoneGate::Refuse(
            "its acceptance criteria were never validated".to_string(),
        )),
        (Some(v), Some(h)) if v != h => Ok(DoneGate::Refuse(
            "it was validated at an earlier commit and HEAD has moved since".to_string(),
        )),
        _ => Ok(DoneGate::Ok),
    }
}

pub fn done_value(conn: &Connection, cfg: &Config, id_or_uuid: &str, force: bool) -> Result<Value> {
    let mut task = db::resolve_task(conn, id_or_uuid)?;
    guard_branch_mutation(conn, id_or_uuid, &task, force)?;

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

    let mut validation_note = Value::Null;
    match done_gate(conn, &task)? {
        DoneGate::Refuse(reason) => {
            let target = task
                .id
                .map(|i| i.to_string())
                .unwrap_or_else(|| task.uuid.to_string()[..8].to_string());
            if !force {
                anyhow::bail!(
                    "Task {} is not validated: {}. Run `validate {}` to prove its acceptance \
                     criteria green, or `done --force` to close without proof.",
                    task.id.unwrap_or(0),
                    reason,
                    target
                );
            }
            validation_note = json!(format!("forced despite unproven work: {reason}"));
        }
        DoneGate::Warn(reason) => validation_note = json!(reason),
        DoneGate::Ok => {}
    }

    if let Some(started) = task.started_at {
        task.time_spent += (Utc::now() - started).num_seconds().max(0);
        task.started_at = None;
    }

    task.status = Status::Completed;
    task.end = Some(Utc::now());
    task.modified = Utc::now();
    db::update_task(conn, &task)?;

    db::repack_ids(conn)?;

    let was_blocking = db::get_blocking(conn, &task.uuid)?;
    for dep_uuid in was_blocking {
        let _ = db::refresh_urgency(conn, &cfg.urgency, &dep_uuid);
    }

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
        "validation": validation_note,
        "auto_memory": db::synthesize_done_memory(conn, &task.uuid, &task.project)
            .unwrap_or(None),
        "hygiene": db::hygiene_pass(conn).map(|h| json!({
            "archived": h.archived,
            "review_pending": h.review_pending,
            "oldest_age_days": h.oldest_age_days,
        })).unwrap_or(Value::Null),
    }))
}

/// Send the anonymous outcome of the task `done` just closed (`v` is its
/// result): how many prior memories it used and whether it was validated.
pub fn report_outcome(conn: &Connection, cfg: &Config, source: Source, v: &Value) {
    let Some(uuid) = v["uuid"]
        .as_str()
        .and_then(|u| uuid::Uuid::parse_str(u).ok())
    else {
        return;
    };
    let n = db::prior_knowledge_used(conn, &uuid).unwrap_or(0);
    let verified = db::get_guide_fields(conn, &uuid)
        .map(|g| g.validated_commit.is_some())
        .unwrap_or(false);
    telemetry::capture_outcome(cfg, source, n, verified);
}

pub fn run(
    conn: &Connection,
    cfg: &Config,
    id_or_uuid: &str,
    force: bool,
    used: &[String],
) -> Result<()> {
    let v = with_citation(conn, id_or_uuid, used, || {
        done_value(conn, cfg, id_or_uuid, force)
    })?;
    report_outcome(conn, cfg, Source::Cli, &v);
    println!(
        "Done: [{}] {}",
        v["project"].as_str().unwrap_or_default(),
        v["description"].as_str().unwrap_or_default()
    );
    print_cited(&v);
    if let Some(rec) = v.get("recurrence").filter(|r| !r.is_null()) {
        println!(
            "♺  Next recurrence: #{} due {}",
            rec.get("id").and_then(|i| i.as_i64()).unwrap_or(0),
            rec.get("due").and_then(|d| d.as_str()).unwrap_or_default()
        );
    }
    if let Some(note) = v.get("validation").and_then(|n| n.as_str()) {
        println!("⚠️  {note}");
    }
    if let Some(label) = v.get("auto_memory").and_then(|m| m.as_str()) {
        println!("🧠 Auto-memory saved: {label} (provisional — review with `sara memories`)");
    }
    if let Some(hyg) = v.get("hygiene").filter(|h| !h.is_null()) {
        let archived: Vec<&str> = json_strs(&hyg["archived"]);
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
#[path = "../../../tests/unit/commands/done/mod__hygiene_tests.rs"]
mod hygiene_tests;

#[cfg(test)]
#[path = "../../../tests/unit/commands/done/mod__validation_gate_tests.rs"]
mod validation_gate_tests;
