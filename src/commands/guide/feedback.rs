use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::commands::shared::{annotation_target, print_json};
use crate::infrastructure::db;

pub fn feedback_value(conn: &Connection, id: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let fb = db::get_open_feedback(conn, &task.uuid)?;
    let arr: Vec<_> = fb
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "text": a.text,
                "target_kind": a.target_kind,
                "target_id": a.target_id,
                "request_revision": a.request_revision,
            })
        })
        .collect();
    Ok(json!({ "task": task.id, "open_feedback": arr }))
}

pub fn feedback(conn: &Connection, id: &str, as_json: bool) -> Result<()> {
    if as_json {
        print_json(&feedback_value(conn, id)?)?;
        return Ok(());
    }

    let task = db::resolve_task(conn, id)?;
    let fb = db::get_open_feedback(conn, &task.uuid)?;

    if fb.is_empty() {
        println!("No open feedback for task {}.", task.id.unwrap_or(0));
        return Ok(());
    }
    for a in &fb {
        let target = annotation_target(a);
        let flag = if a.request_revision { " ⟳" } else { "" };
        println!("#{}{}{}: {}", a.id, target, flag, a.text);
    }
    Ok(())
}

pub fn resolve_value(
    conn: &Connection,
    feedback_id: i64,
    run_id: Option<i64>,
) -> Result<serde_json::Value> {
    if !db::resolve_annotation(conn, feedback_id, run_id)? {
        anyhow::bail!("No feedback with id {feedback_id}");
    }
    Ok(json!({ "feedback_id": feedback_id, "resolved": true, "run_id": run_id }))
}

pub fn resolve(conn: &Connection, feedback_id: i64, run_id: Option<i64>) -> Result<()> {
    resolve_value(conn, feedback_id, run_id)?;
    println!("Resolved feedback #{feedback_id}.");
    Ok(())
}
