use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::json;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Task;

mod types;
use types::PlanInput;

pub fn import(conn: &Connection, cfg: &Config, source: &str) -> Result<()> {
    let raw = if source == "-" {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        std::fs::read_to_string(source)
            .with_context(|| format!("could not read plan file: {source}"))?
    };

    let v = import_raw(conn, cfg, &raw)?;
    println!(
        "Imported {} task(s) into project '{}'.",
        v.get("created").and_then(|c| c.as_u64()).unwrap_or(0),
        v.get("project").and_then(|p| p.as_str()).unwrap_or("")
    );
    Ok(())
}

pub fn import_raw(conn: &Connection, cfg: &Config, raw: &str) -> Result<serde_json::Value> {
    let plan: PlanInput = serde_json::from_str(raw).context("plan JSON was invalid")?;
    if plan.tasks.is_empty() {
        anyhow::bail!("plan contains no tasks");
    }

    let (default_project, _path) =
        crate::infrastructure::project::detect_current_project(conn, cfg)?;
    let project = plan.project.clone().unwrap_or(default_project);

    let tx = conn.unchecked_transaction()?;
    let mut key_to_uuid: std::collections::HashMap<String, uuid::Uuid> =
        std::collections::HashMap::new();
    let mut created = 0u32;

    for pt in &plan.tasks {
        let mut task = Task::new(pt.description.clone(), project.clone());
        if let Some(p) = &pt.priority {
            task.priority = p.parse().ok();
        }
        task.tags = pt.tags.clone();
        db::insert_task(&tx, &mut task)?;
        created += 1;

        if let Some(a) = pt.assignment.as_deref().filter(|s| !s.trim().is_empty()) {
            db::set_assignment(&tx, &task.uuid, a.trim())?;
        }
        if let Some(r) = pt.rationale.as_deref().filter(|s| !s.trim().is_empty()) {
            db::set_rationale(&tx, &task.uuid, r.trim())?;
        }
        for s in &pt.steps {
            if !s.trim().is_empty() {
                db::add_step(
                    &tx,
                    &task.uuid,
                    s.trim(),
                    None,
                    db::STEP_KIND_STEP,
                    "ai",
                    None,
                )?;
            }
        }
        for a in &pt.acceptance {
            if !a.trim().is_empty() {
                db::add_step(
                    &tx,
                    &task.uuid,
                    a.trim(),
                    None,
                    db::STEP_KIND_ACCEPTANCE,
                    "ai",
                    None,
                )?;
            }
        }
        for (kind, items) in [("finding", &pt.findings), ("constraint", &pt.constraints)] {
            for text in items {
                if !text.trim().is_empty() {
                    db::add_annotation_full(
                        &tx,
                        &task.uuid,
                        text.trim(),
                        kind,
                        "ai",
                        None,
                        None,
                        false,
                    )?;
                }
            }
        }
        for f in &pt.files {
            if !f.path.trim().is_empty() {
                db::add_task_file(
                    &tx,
                    &task.uuid,
                    f.path.trim(),
                    db::SOURCE_SUGGESTED,
                    f.reason.as_deref(),
                    f.symbol.as_deref(),
                    f.line_start,
                    f.line_end,
                )?;
            }
        }

        if let Some(key) = &pt.key {
            key_to_uuid.insert(key.clone(), task.uuid);
        }
    }

    for pt in &plan.tasks {
        let Some(key) = &pt.key else { continue };
        let Some(from) = key_to_uuid.get(key) else {
            continue;
        };
        for dep in &pt.depends_on {
            if let Some(target) = key_to_uuid.get(dep) {
                db::add_dependency(&tx, from, target)?;
            } else if let Ok(existing) = db::resolve_task(&tx, dep) {
                db::add_dependency(&tx, from, &existing.uuid)?;
            }
        }
    }

    tx.commit()?;
    Ok(json!({ "created": created, "project": project }))
}

pub fn show_value(conn: &Connection, id: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let order = db::dependency_closure(conn, &task.uuid)?;
    let mut arr = vec![];
    for uuid in &order {
        if let Ok(g) = db::guide_json(conn, uuid) {
            arr.push(g);
        }
    }
    Ok(json!({ "briefing": arr }))
}

pub fn show(conn: &Connection, _cfg: &Config, id: &str, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(&show_value(conn, id)?)?);
        return Ok(());
    }

    let task = db::resolve_task(conn, id)?;
    let order = db::dependency_closure(conn, &task.uuid)?;

    println!(
        "Briefing for task {} (dependency-ordered):\n",
        task.id.unwrap_or(0)
    );
    for uuid in &order {
        let g = match db::guide_json(conn, uuid) {
            Ok(g) => g,
            Err(_) => continue,
        };
        let id = g.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let desc = g.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let status = g.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let marker = if *uuid == task.uuid { "▶" } else { "·" };
        println!("{marker} [{status}] task {id}: {desc}");
        if let Some(rationale) = g.get("rationale").and_then(|v| v.as_str()) {
            println!("    why: {rationale}");
        }
        if let Some(steps) = g.get("steps").and_then(|v| v.as_array()) {
            for (i, s) in steps.iter().enumerate() {
                let text = s.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let done = s.get("done").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
                let mark = if done { "x" } else { " " };
                println!("    [{mark}] {}. {text}", i + 1);
            }
        }
        println!();
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/plan/mod.rs"]
mod tests;
