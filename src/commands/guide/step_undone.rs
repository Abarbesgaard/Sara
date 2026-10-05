use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use super::support::kind_arg;
use crate::commands::shared::print_json;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

pub fn step_undone_value(
    conn: &Connection,
    id: &str,
    n: usize,
    kind: Option<&str>,
) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let kind = kind_arg(kind);
    let step_id = db::step_id_by_index(conn, &task.uuid, kind, n)?;
    db::set_step_done(conn, step_id, false, None, None)?;
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "kind": kind,
        "index": n,
        "done": false,
    }))
}

pub fn step_undone_by_id_value(conn: &Connection, step_id: i64) -> Result<serde_json::Value> {
    let (uuid, kind, index) = db::locate_step(conn, step_id)?;
    step_undone_value(conn, &uuid.to_string(), index, Some(&kind))
}

pub fn step_undone(
    conn: &Connection,
    _cfg: &Config,
    id: &str,
    n: usize,
    kind: Option<&str>,
    as_json: bool,
) -> Result<()> {
    let v = step_undone_value(conn, id, n, kind)?;
    if as_json {
        print_json(&v)?;
        return Ok(());
    }
    println!(
        "Reopened {} {} of task {}.",
        v["kind"].as_str().unwrap_or("step"),
        n,
        v["task"].as_i64().unwrap_or(0)
    );
    Ok(())
}
