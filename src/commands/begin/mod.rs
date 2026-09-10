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
//!
//! Sequence:
//!   1. create the task from the description (tags / files / priority),
//!   2. set its assignment (the originating request) and, if given, its why,
//!   3. register an acceptance criterion — OPTIONAL: warn but proceed if none,
//!   4. seed the first step — recall prior art (the agent runs `recall` itself),
//!   5. print the task, its criteria, the seeded recall step, and the next cursor.

use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::commands;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::telemetry::{self, Source, Span};

/// The text of the first step `begin` seeds on every task: an explicit,
/// agent-run recall. `begin` no longer recalls itself — this step makes prior
/// art the agent's own first deliberate act, surfaced by `next` before any
/// other work so it shapes the task instead of being skimmed after the fact.
const RECALL_STEP_TEXT: &str = "Recall prior art before doing anything else";

/// The intent bound to the seeded recall step — it forces the agent to decide
/// WHAT to remember for, not merely to fire a query.
const RECALL_STEP_INTENT: &str = "Before investigating or editing, decide what prior knowledge bears on this task — the patterns, prior fixes, gotchas, and conventions it might repeat — then call `recall` with a query aimed at exactly that. Record which memories apply (or are deliberately rejected, and why) when you close this step. Do this first so prior art shapes the work rather than being consulted after the fact.";

/// Emit one nested telemetry event for a folded internal operation of `begin`
/// and append it to the returned event log. `begin` is a composition — this
/// makes each internal step a discrete, ordered, recorded event (event-sourcing
/// inspired) rather than a single opaque call. The `via_begin` flag marks the
/// event as part of a `begin` composition so it stays filterable, and the name
/// carries the `mcp ` prefix when the founding call came in over MCP.
///
/// Every event is stamped with the composition's `trace_id` and its 0-based
/// `seq` (derived from the log length so far), so the whole fan-out reconstructs
/// as one correlated, ordered trace instead of records that merely happen to be
/// adjacent in time. `n` carries an optional non-identifying outcome COUNT for
/// the op (e.g. how many prior memories a `recall` matched) — a metric, never
/// content.
fn emit_folded(
    cfg: &Config,
    source: Source,
    trace_id: &str,
    log: &mut Vec<Value>,
    op: &str,
    dur_ms: u64,
    n: Option<u64>,
) {
    let seq = log.len() as u32;
    let name = match source {
        Source::Mcp => format!("mcp {op}"),
        Source::Cli => op.to_string(),
    };
    telemetry::capture_span(
        cfg,
        source,
        &name,
        &["via_begin".to_string()],
        dur_ms,
        &Ok::<(), anyhow::Error>(()),
        None,
        Some(Span { trace_id, seq, n }),
    );
    let mut entry = json!({ "op": op, "seq": seq, "duration_ms": dur_ms });
    if let Some(n) = n {
        entry["n"] = json!(n);
    }
    log.push(entry);
}

/// Compose the full "start a task" flow and return a structured result. Every
/// sub-step reuses the same value function the standalone command calls, so
/// `begin` can never drift from `add`/`recall`/`check`/… behaviour.
///
/// Each folded internal operation also emits its OWN nested telemetry event
/// (flagged `via_begin`) and is recorded in the returned `folded` event log, so
/// the whole internal composition of a single `begin` is observable — the
/// `source` decides whether those events are named `mcp <op>` or plain `<op>`.
#[allow(clippy::too_many_arguments)]
pub fn begin_value(
    conn: &Connection,
    cfg: &Config,
    source: Source,
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
    // The ordered event log of every folded internal operation this `begin`
    // performs, returned to the caller and mirrored into telemetry.
    let mut folded: Vec<Value> = Vec::new();
    // One correlation id for this whole `begin` composition. Every folded event
    // carries it (and an ordinal `seq`), so the fan-out reconstructs as a single
    // ordered trace. Anonymous and ephemeral — a fresh random id per `begin`.
    let trace_id = uuid::Uuid::new_v4().to_string();

    // 1. Create the task.
    let t = std::time::Instant::now();
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
    emit_folded(
        cfg,
        source,
        &trace_id,
        &mut folded,
        "add",
        t.elapsed().as_millis() as u64,
        None,
    );
    let id_num = created["id"].as_i64().unwrap_or_default();
    let id = id_num.to_string();

    // 1b. Attach any declared files as task anchors. begin no longer folds them
    //     into a recall query (it does not recall); instead it records them as
    //     the task's code anchors so the agent's own recall step, `next`'s
    //     relevant-memory block, and later work all have the touched files as
    //     first-class context.
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
        let t = std::time::Instant::now();
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
        emit_folded(
            cfg,
            source,
            &trace_id,
            &mut folded,
            "files",
            t.elapsed().as_millis() as u64,
            Some(anchor_files.len() as u64),
        );
    }

    // 2. Assignment — the originating request. Defaults to the description so
    //    the task always records what was actually asked for.
    let assignment_text = assignment
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| description.trim());
    let t = std::time::Instant::now();
    commands::guide::assignment_value(conn, &id, assignment_text)?;
    emit_folded(
        cfg,
        source,
        &trace_id,
        &mut folded,
        "assignment",
        t.elapsed().as_millis() as u64,
        None,
    );

    // 3. Rationale (why this task exists), when supplied.
    let rationale_text = rationale.map(str::trim).filter(|s| !s.is_empty());
    if let Some(why) = rationale_text {
        let t = std::time::Instant::now();
        commands::guide::rationale_value(conn, &id, why)?;
        emit_folded(
            cfg,
            source,
            &trace_id,
            &mut folded,
            "rationale",
            t.elapsed().as_millis() as u64,
            None,
        );
    }

    // 4. Acceptance criterion — OPTIONAL. A task without a definition of done is
    //    allowed to proceed, but we warn so it is a deliberate choice, not a
    //    silent gap.
    let acceptance = match check.map(str::trim).filter(|s| !s.is_empty()) {
        Some(text) => {
            let t = std::time::Instant::now();
            let c = commands::guide::check_value(
                conn,
                &id,
                text,
                None,
                Some("acceptance"),
                Some("agent"),
                verify,
            )?;
            emit_folded(
                cfg,
                source,
                &trace_id,
                &mut folded,
                "check",
                t.elapsed().as_millis() as u64,
                None,
            );
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

    // 5. Seed the first step: an explicit, agent-run recall. begin does NOT
    //    recall here — it appends a checklist step directing the agent to decide
    //    what prior art matters and call `recall` itself. Seeded first (before
    //    any workflow steps a skill may later add), it is the cursor `next`
    //    returns, so prior art is recalled early yet purposefully — the agent's
    //    own act, not a mechanical description-derived query.
    let t = std::time::Instant::now();
    let recall_step = commands::guide::check_value(
        conn,
        &id,
        RECALL_STEP_TEXT,
        Some(RECALL_STEP_INTENT),
        Some("step"),
        Some("sara"),
        None,
    )?;
    emit_folded(
        cfg,
        source,
        &trace_id,
        &mut folded,
        "step",
        t.elapsed().as_millis() as u64,
        None,
    );

    // 7. The execution cursor — where the work goes next.
    let t = std::time::Instant::now();
    let next = commands::guide::next_value(conn, &id)?;
    emit_folded(
        cfg,
        source,
        &trace_id,
        &mut folded,
        "next",
        t.elapsed().as_millis() as u64,
        None,
    );

    Ok(json!({
        "task": id_num,
        "begin_id": trace_id,
        "uuid": created["uuid"],
        "project": created["project"],
        "description": description.trim(),
        "assignment": assignment_text,
        "rationale": rationale_text,
        "acceptance": acceptance,
        "recall_step": recall_step,
        "next": next,
        "folded": folded,
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
        Source::Cli,
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

    let task = v["task"].as_i64().unwrap_or_default();
    let project = v["project"].as_str().unwrap_or("");
    println!(
        "Started task {task} in {project}: {}",
        v["description"].as_str().unwrap_or("")
    );

    match v["acceptance"].as_object() {
        Some(a) => println!("  acceptance: {}", a["text"].as_str().unwrap_or("")),
        None => println!("  acceptance: (none — add one with `sara check`)"),
    }

    // begin seeds an explicit recall step (it does NOT recall itself); `next`
    // below points at it. Surface it plainly so the agent recalls before acting.
    println!("  step 1: {RECALL_STEP_TEXT} — run `sara recall` before you act");

    if let Some(next) = v["next"].as_object() {
        if next.get("done").and_then(Value::as_bool) == Some(true) {
            println!("  next: no steps yet — add them with `sara check <id> \"…\"`");
        } else if let Some(text) = next.get("text").and_then(Value::as_str) {
            println!("  next: {text}");
        } else {
            println!("  next: `sara next {task}`");
        }
    }

    for w in v["warnings"].as_array().cloned().unwrap_or_default() {
        if let Some(w) = w.as_str() {
            eprintln!("warning: {w}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db;

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn begin_founds_a_task_with_assignment_and_next_cursor() {
        let conn = db::open_in_memory_for_test();
        let v = begin_value(
            &conn,
            &cfg(),
            Source::Cli,
            "Fix the failing build",
            &["ci".to_string()],
            &[],
            Some("proj"),
            None,
            None,
            Some("the restore is red on NU1608"),
            Some("dotnet build is green"),
            Some("dotnet build"),
        )
        .expect("begin succeeds");

        assert!(v["task"].as_i64().unwrap() > 0, "a task id is minted");
        // Assignment defaults to the description when not given explicitly.
        assert_eq!(v["assignment"].as_str().unwrap(), "Fix the failing build");
        assert_eq!(
            v["rationale"].as_str().unwrap(),
            "the restore is red on NU1608"
        );
        // The acceptance criterion is registered and carries its verify command.
        assert_eq!(
            v["acceptance"]["kind"].as_str().unwrap(),
            db::STEP_KIND_ACCEPTANCE
        );
        // begin does NOT recall — it seeds a first recall STEP instead.
        assert!(
            v.get("recall").is_none() && v.get("finding").is_none(),
            "begin no longer auto-recalls: no recall/finding keys, got {v}"
        );
        assert_eq!(
            v["recall_step"]["kind"].as_str().unwrap(),
            db::STEP_KIND_STEP,
            "a checklist step is seeded"
        );
        // The seeded recall step is the execution cursor `next` returns, so
        // prior art is the first thing the agent is directed to do.
        assert_eq!(
            v["next"]["text"].as_str().unwrap(),
            RECALL_STEP_TEXT,
            "the recall step is the next cursor, got {}",
            v["next"]
        );
    }

    #[test]
    fn missing_acceptance_warns_but_still_founds_the_task() {
        let conn = db::open_in_memory_for_test();
        let v = begin_value(
            &conn,
            &cfg(),
            Source::Cli,
            "Add a config flag",
            &[],
            &[],
            Some("proj"),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("begin succeeds without acceptance");

        assert!(v["task"].as_i64().unwrap() > 0);
        assert!(
            v["acceptance"].is_null(),
            "no acceptance criterion recorded"
        );
        let warnings: Vec<String> = v["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|w| w.as_str().map(str::to_string))
            .collect();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("no acceptance criterion")),
            "an absent definition of done is warned, not blocked: {warnings:?}"
        );
    }

    #[test]
    fn begin_records_every_folded_operation_as_an_ordered_event() {
        // `begin` is a composition: internally it runs add, assignment,
        // rationale, check, the seeded recall step, and next. Each folded
        // operation is surfaced as its own ordered event (event-sourcing
        // inspired) so the whole internal fan-out of a single `begin` is
        // observable, both in the returned `folded` log and — for real runs —
        // in telemetry.
        let conn = db::open_in_memory_for_test();
        let v = begin_value(
            &conn,
            &cfg(),
            Source::Cli,
            "Fix the failing build",
            &["ci".to_string()],
            &[],
            Some("proj"),
            None,
            None,
            Some("the restore is red on NU1608"),
            Some("dotnet build is green"),
            Some("dotnet build"),
        )
        .expect("begin succeeds");

        // begin does not recall — no recall block is emitted.
        assert!(
            v.get("recall").is_none(),
            "begin no longer auto-recalls: {v}"
        );

        // With assignment, rationale and check all supplied, every folded
        // operation is recorded, in composition order. The seeded recall `step`
        // replaces the former `recall`/`annotate` ops.
        let ops: Vec<String> = v["folded"]
            .as_array()
            .expect("folded event log is an array")
            .iter()
            .map(|e| e["op"].as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(
            ops,
            vec!["add", "assignment", "rationale", "check", "step", "next",],
            "every folded internal operation that ran is logged in order: {ops:?}"
        );
        // Each event carries a duration.
        for e in v["folded"].as_array().unwrap() {
            assert!(
                e["duration_ms"].is_u64(),
                "each folded event records its duration: {e}"
            );
        }

        // The whole composition shares one correlation id and the events carry a
        // contiguous 0-based `seq`, so the fan-out reconstructs as one ordered
        // trace rather than timestamp-adjacent records.
        assert!(
            v["begin_id"].as_str().is_some_and(|s| !s.is_empty()),
            "begin returns a trace/correlation id: {}",
            v["begin_id"]
        );
        let seqs: Vec<u64> = v["folded"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["seq"].as_u64().expect("each folded event has a seq"))
            .collect();
        assert_eq!(
            seqs,
            (0..seqs.len() as u64).collect::<Vec<_>>(),
            "folded events carry a contiguous 0-based ordinal: {seqs:?}"
        );
        // The seeded recall step is folded in as a `step` op.
        let step = v["folded"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["op"] == "step")
            .expect("the seeded recall step is folded");
        assert!(
            step["duration_ms"].is_u64(),
            "the folded step reports its own duration: {step}"
        );
    }

    #[test]
    fn begin_folded_log_skips_operations_that_did_not_run() {
        // When rationale and check are absent, their folded events are not
        // fabricated — the log reflects only what actually ran. The recall
        // `step` is always seeded, so it is always present.
        let conn = db::open_in_memory_for_test();
        let v = begin_value(
            &conn,
            &cfg(),
            Source::Cli,
            "Add a config flag",
            &[],
            &[],
            Some("proj"),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("begin succeeds without acceptance");

        let ops: Vec<String> = v["folded"]
            .as_array()
            .expect("folded event log is an array")
            .iter()
            .map(|e| e["op"].as_str().unwrap_or_default().to_string())
            .collect();
        assert!(
            ops.contains(&"add".to_string()) && ops.contains(&"step".to_string()),
            "the always-run operations are logged: {ops:?}"
        );
        assert!(
            !ops.contains(&"rationale".to_string()) && !ops.contains(&"check".to_string()),
            "operations that did not run are not logged: {ops:?}"
        );
    }
}
