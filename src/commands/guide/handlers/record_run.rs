use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::infrastructure::db;

#[allow(clippy::too_many_arguments)]
pub fn record_run_value(
    conn: &Connection,
    id: &str,
    kind: &str,
    model: Option<&str>,
    provider: Option<&str>,
    prompt: Option<&str>,
    response: Option<&str>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    total_tokens: Option<i64>,
) -> Result<serde_json::Value> {
    anyhow::ensure!(!kind.trim().is_empty(), "kind cannot be empty");
    let task = db::resolve_task(conn, id)?;
    let run_id = db::record_ai_run(
        conn,
        &task.uuid,
        kind,
        model,
        provider,
        prompt,
        response,
        prompt_tokens,
        completion_tokens,
        total_tokens,
    )?;
    Ok(json!({
        "task": task.id,
        "run_id": run_id,
        "kind": kind,
        "model": model,
        "provider": provider,
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": total_tokens,
    }))
}

#[allow(clippy::too_many_arguments)]
pub fn record_run(
    conn: &Connection,
    id: &str,
    kind: &str,
    model: Option<&str>,
    provider: Option<&str>,
    prompt: Option<&str>,
    response: Option<&str>,
) -> Result<()> {
    let v = record_run_value(
        conn, id, kind, model, provider, prompt, response, None, None, None,
    )?;
    println!(
        "Recorded {} run #{} on task {}.",
        v["kind"].as_str().unwrap_or_default(),
        v["run_id"].as_i64().unwrap_or(0),
        v["task"].as_i64().unwrap_or(0),
    );
    Ok(())
}
