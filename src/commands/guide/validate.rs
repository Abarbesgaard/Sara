use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use super::gate::run_acceptance_gate;
use super::types::GateOutput;
use crate::commands::shared::{guard_branch_mutation, project_head};
use crate::infrastructure::db;

pub fn validate_value(
    conn: &Connection,
    id: &str,
    skip_gate: bool,
    mode: GateOutput,
    fresh: bool,
) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    guard_branch_mutation(conn, id, &task, false)?;
    let head = project_head(conn, &task.project)
        .ok_or_else(|| anyhow::anyhow!("task's project is not in a git repo"))?;

    let mut gate_ran = 0usize;
    let mut gate_cached = 0usize;
    if !skip_gate {
        let gate = run_acceptance_gate(conn, id, mode, fresh)?;
        if !gate.is_green() {
            anyhow::bail!(
                "validate refused — acceptance gate is red: {}.{} \
                 Fix and re-run, or `validate --no-run` to stamp without proof (discouraged).",
                gate.reason(),
                gate.failure_detail()
            );
        }
        gate_ran = gate.ran;
        gate_cached = gate.cached;
    }

    db::set_validated(conn, &task.uuid, &head)?;

    let open_steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?
        .iter()
        .filter(|s| !s.done)
        .count();

    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "validated_commit": head,
        "gate_skipped": skip_gate,
        "open_steps": open_steps,
        "criteria_ran": gate_ran,
        "criteria_cached": gate_cached,
    }))
}

pub fn validate(conn: &Connection, id: &str, no_run: bool, fresh: bool) -> Result<()> {
    if no_run {
        eprintln!(
            "⚠ validate --no-run: stamping WITHOUT running the acceptance gate — \
             'validated' will not be backed by a passing command."
        );
    }
    let v = validate_value(conn, id, no_run, GateOutput::Stream, fresh)?;
    println!(
        "Stamped task {} validated @ {}.",
        v["task"].as_i64().unwrap_or(0),
        v["validated_commit"].as_str().unwrap_or_default()
    );
    let cached = v["criteria_cached"].as_u64().unwrap_or(0);
    if cached > 0 {
        println!(
            "  {cached} criterion/criteria reused from cache (already proven at this \
             commit) — use `--fresh` to force a full re-run."
        );
    }
    let open = v["open_steps"].as_u64().unwrap_or(0);
    if open > 0 {
        println!(
            "  advisory: {open} checklist step(s) still open — acceptance is green, \
             but the plan isn't fully ticked (see `sara steps {}`).",
            v["task"].as_i64().unwrap_or(0)
        );
    }
    Ok(())
}
