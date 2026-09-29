//! `sara begin` — the single entry point for starting a task.
//!
//! One call founds the task AND seeds the first step of the flow: an explicit,
//! agent-run recall. `begin` does NOT recall itself — instead it appends a first
//! checklist step directing the agent to decide what prior art bears on the task
//! and call `recall` with a purposeful query. This keeps prior art early in
//! context (it is the very first step `next` surfaces) while making the recall
//! the agent's own deliberate act — it must state WHAT it is trying to remember
//! for — rather than a mechanical, description-derived query it skims past.
//!
//! It is a THIN composition of the existing value functions (`add`,
//! `assignment`, `rationale`, `check`, `next`) plus one seeded step — it
//! introduces no new storage and stays deliberately skill/tooling-agnostic: it
//! speaks only of tasks, acceptance criteria, steps and memories, never of any
//! particular workflow, rite, or agent methodology.

use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::commands;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

mod render;

/// The text of the first step `begin` seeds on every task: an explicit,
/// agent-run recall. `begin` no longer recalls itself — this step makes prior
/// art the agent's own first deliberate act, surfaced by `next` before any
/// other work so it shapes the task instead of being skimmed after the fact.
pub(super) const RECALL_STEP_TEXT: &str = "Recall prior art before doing anything else";

/// The intent bound to the seeded recall step — it forces the agent to decide
/// WHAT to remember for, not merely to fire a query.
const RECALL_STEP_INTENT: &str = "Before investigating or editing, decide what prior knowledge bears on this task — the patterns, prior fixes, gotchas, and conventions it might repeat — then call `recall` with a query aimed at exactly that. Record which memories apply (or are deliberately rejected, and why) when you close this step. Do this first so prior art shapes the work rather than being consulted after the fact.";

/// Compose the full "start a task" flow and return a structured result. Every
/// sub-step reuses the same value function the standalone command calls, so
/// `begin` can never drift from `add`/`recall`/`check`/… behaviour.
#[allow(clippy::too_many_arguments)]
pub fn begin_value(
    conn: &Connection,
    cfg: &Config,
    description: &str,
    tags: &[String],
    files: &[String],
    project: Option<&str>,
    priority: Option<&str>,
    assignment: Option<&str>,
    rationale: Option<&str>,
    check: Option<&str>,
    verify: Option<&str>,
) -> Result<Value> {
    if description.trim().is_empty() {
        anyhow::bail!("Task description cannot be empty");
    }

    let mut warnings: Vec<String> = Vec::new();

    let created = commands::add::run_value(
        conn,
        cfg,
        &[description.to_string()],
        project,
        priority,
        tags,
        None,
        &[],
        &[],
        &[],
        &[],
    )?;
    let id_num = created["id"].as_i64().unwrap_or_default();
    let id = id_num.to_string();

    // Attach any declared files as task anchors. begin no longer folds them into
    // a recall query (it does not recall); it records them as the task's code
    // anchors so the agent's recall step, `next`'s relevant-memory block, and
    // later work all have the touched files as first-class context.
    let anchor_files: Vec<&str> = files
        .iter()
        .map(|f| f.trim())
        .filter(|f| !f.is_empty())
        .collect();
    if !anchor_files.is_empty()
        && let Ok(uuid) = created["uuid"]
            .as_str()
            .unwrap_or_default()
            .parse::<uuid::Uuid>()
    {
        for path in &anchor_files {
            db::add_task_file(
                conn,
                &uuid,
                path,
                "agent",
                Some("declared at begin"),
                None,
                None,
                None,
            )?;
        }
    }

    // Assignment defaults to the description so the task always records what was
    // actually asked for.
    let assignment_text = assignment
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| description.trim());
    commands::guide::assignment_value(conn, &id, assignment_text)?;

    let rationale_text = rationale.map(str::trim).filter(|s| !s.is_empty());
    if let Some(why) = rationale_text {
        commands::guide::rationale_value(conn, &id, why)?;
    }

    // Acceptance is OPTIONAL — proceed without it, but warn so the gap is a
    // deliberate choice.
    let acceptance = match check.map(str::trim).filter(|s| !s.is_empty()) {
        Some(text) => {
            let c = commands::guide::check_value(
                conn,
                &id,
                text,
                None,
                Some("acceptance"),
                Some("agent"),
                verify,
            )?;
            if let Some(w) = c["warning"].as_str() {
                warnings.push(w.to_string());
            }
            Some(c)
        }
        None => {
            warnings.push(
                "no acceptance criterion — define done with \
                 `sara check <id> \"…\" --kind acceptance --verify \"<cmd>\"`"
                    .to_string(),
            );
            None
        }
    };

    // Seed the recall step FIRST — before any workflow steps a skill may later
    // add — so it is the cursor `next` returns and prior art is recalled early
    // yet purposefully. begin does not recall here; the agent runs `recall`.
    let recall_step = commands::guide::check_value(
        conn,
        &id,
        RECALL_STEP_TEXT,
        Some(RECALL_STEP_INTENT),
        Some("step"),
        Some("sara"),
        None,
    )?;

    let next = commands::guide::next_value(conn, &id)?;

    Ok(json!({
        "task": id_num,
        "uuid": created["uuid"],
        "project": created["project"],
        "description": description.trim(),
        "assignment": assignment_text,
        "rationale": rationale_text,
        "acceptance": acceptance,
        "recall_step": recall_step,
        "next": next,
        "warnings": warnings,
    }))
}

/// `sara begin` — CLI entry: found the task, seed the recall step, print a
/// summary (or the raw JSON with `--json`). It does NOT recall — the seeded
/// first step directs the agent to run `recall` itself.
#[allow(clippy::too_many_arguments)]
pub fn run(
    conn: &Connection,
    cfg: &Config,
    description: &str,
    tags: &[String],
    files: &[String],
    project: Option<&str>,
    priority: Option<&str>,
    assignment: Option<&str>,
    rationale: Option<&str>,
    check: Option<&str>,
    verify: Option<&str>,
    as_json: bool,
) -> Result<()> {
    let v = begin_value(
        conn,
        cfg,
        description,
        tags,
        files,
        project,
        priority,
        assignment,
        rationale,
        check,
        verify,
    )?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }

    render::print_begin(&v);
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/begin/mod.rs"]
mod tests;
