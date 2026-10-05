use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use super::support::kind_arg;
use crate::commands::shared::{insight, print_json, project_head};
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

pub fn step_done_value(
    conn: &Connection,
    id: &str,
    n: usize,
    result: Option<&str>,
    kind: Option<&str>,
) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let kind = kind_arg(kind);
    let step_id = db::step_id_by_index(conn, &task.uuid, kind, n)?;
    let commit = project_head(conn, &task.project);
    db::set_step_done(conn, step_id, true, result, commit.as_deref())?;
    let activated = db::ensure_started(conn, &task.uuid)?;
    let related = match result {
        Some(r) if !r.trim().is_empty() => insight::related_findings(conn, &task.uuid, r, None),
        _ => Vec::new(),
    };
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "kind": kind,
        "index": n,
        "done": true,
        "commit": commit,
        "activated": activated,
        "related_findings": insight::related_findings_json(&related),
    }))
}

pub fn step_done_current_value(
    conn: &Connection,
    id: &str,
    result: Option<&str>,
    kind: Option<&str>,
) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let kind_str = kind_arg(kind);
    let steps = db::get_steps(conn, &task.uuid, kind_str)?;
    let n = steps
        .iter()
        .position(|s| !s.done)
        .map(|i| i + 1)
        .ok_or_else(|| anyhow::anyhow!("no incomplete {kind_str} remains on task {id}"))?;
    step_done_value(conn, id, n, result, kind)
}

pub fn step_done_by_id_value(
    conn: &Connection,
    step_id: i64,
    result: Option<&str>,
) -> Result<serde_json::Value> {
    let (uuid, kind, index) = db::locate_step(conn, step_id)?;
    step_done_value(conn, &uuid.to_string(), index, result, Some(&kind))
}

pub fn step_done(
    conn: &Connection,
    _cfg: &Config,
    id: &str,
    n: usize,
    result: Option<&str>,
    kind: Option<&str>,
    as_json: bool,
) -> Result<()> {
    let v = step_done_value(conn, id, n, result, kind)?;
    if as_json {
        print_json(&v)?;
        return Ok(());
    }
    let commit_suffix = v
        .get("commit")
        .and_then(|c| c.as_str())
        .map(|c| format!(" @ {c}"))
        .unwrap_or_default();
    println!(
        "Marked {} {} of task {} done{}.",
        v.get("kind").and_then(|k| k.as_str()).unwrap_or("step"),
        n,
        v.get("task").and_then(|t| t.as_i64()).unwrap_or(0),
        commit_suffix
    );
    insight::print_related_json(&v, false);
    Ok(())
}
