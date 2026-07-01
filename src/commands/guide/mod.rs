use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;

/// Resolve the git HEAD for the task's project, if it lives in a repo.
fn project_head(conn: &Connection, project: &str) -> Option<String> {
    let proj = db::get_project(conn, project).ok().flatten()?;
    let path = proj.path?;
    crate::infrastructure::git::head_commit(std::path::Path::new(&path))
}

fn kind_arg(kind: Option<&str>) -> &str {
    match kind {
        Some("acceptance") => db::STEP_KIND_ACCEPTANCE,
        _ => db::STEP_KIND_STEP,
    }
}

/// Structured form of the execution cursor (first not-done step). Shared by the
/// `--json` CLI path and the MCP `next` tool so there is a single serializer.
pub fn next_value(conn: &Connection, id: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    let next = steps.iter().enumerate().find(|(_, s)| !s.done);
    Ok(match next {
        Some((i, s)) => json!({
            "task": task.id,
            "index": i + 1,
            "total": steps.len(),
            "text": s.text,
            "intent": s.intent,
            "verify_cmd": s.verify_cmd,
            "source": s.source,
        }),
        None => json!({ "task": task.id, "done": true, "total": steps.len() }),
    })
}

/// `sara next` — the execution cursor: first not-done step.
pub fn next(conn: &Connection, _cfg: &Config, id: &str, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(&next_value(conn, id)?)?);
        return Ok(());
    }

    let task = db::resolve_task(conn, id)?;
    let steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    let next = steps.iter().enumerate().find(|(_, s)| !s.done);

    match next {
        Some((i, s)) => {
            println!("Next step {}/{}: {}", i + 1, steps.len(), s.text);
            if let Some(intent) = &s.intent {
                println!("  intent: {intent}");
            }
            if let Some(v) = &s.verify_cmd {
                println!("  verify: {v}");
            }
        }
        None if steps.is_empty() => println!("No steps defined for task {}.", task.id.unwrap_or(0)),
        None => println!("All steps complete for task {}.", task.id.unwrap_or(0)),
    }
    Ok(())
}

/// Structured form of the ordered steps. Shared by the `--json` CLI path and the
/// MCP `steps` tool.
pub fn steps_value(conn: &Connection, id: &str, until: Option<usize>) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let mut steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    if let Some(n) = until {
        steps.truncate(n);
    }
    let arr: Vec<_> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            json!({
                "index": i + 1,
                "text": s.text,
                "intent": s.intent,
                "done": s.done,
                "source": s.source,
                "verify_cmd": s.verify_cmd,
                "result": s.result,
            })
        })
        .collect();
    Ok(json!({ "task": task.id, "steps": arr }))
}

/// `sara steps [--until N]` — ordered steps for incremental execution.
pub fn steps(
    conn: &Connection,
    _cfg: &Config,
    id: &str,
    until: Option<usize>,
    as_json: bool,
) -> Result<()> {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&steps_value(conn, id, until)?)?
        );
        return Ok(());
    }

    let task = db::resolve_task(conn, id)?;
    let mut steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    if let Some(n) = until {
        steps.truncate(n);
    }

    if steps.is_empty() {
        println!("No steps defined for task {}.", task.id.unwrap_or(0));
        return Ok(());
    }
    for (i, s) in steps.iter().enumerate() {
        let mark = if s.done { "[x]" } else { "[ ]" };
        let badge = if s.source == "ai" { " (ai)" } else { "" };
        println!("{} {}. {}{}", mark, i + 1, s.text, badge);
        if let Some(intent) = &s.intent {
            println!("      {intent}");
        }
    }
    Ok(())
}

/// Mark step `n` done and return a structured record of the change. Shared by the
/// CLI `step done` command and the MCP `step_done` tool (which cannot print).
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
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "kind": kind,
        "index": n,
        "done": true,
        "commit": commit,
    }))
}

/// `sara step done <id> <n>` — record completion of a step.
pub fn step_done(
    conn: &Connection,
    _cfg: &Config,
    id: &str,
    n: usize,
    result: Option<&str>,
    kind: Option<&str>,
) -> Result<()> {
    let v = step_done_value(conn, id, n, result, kind)?;
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
    Ok(())
}

/// `sara step undone <id> <n>` — reopen a step.
pub fn step_undone(
    conn: &Connection,
    _cfg: &Config,
    id: &str,
    n: usize,
    kind: Option<&str>,
) -> Result<()> {
    let task = db::resolve_task(conn, id)?;
    let kind = kind_arg(kind);
    let step_id = db::step_id_by_index(conn, &task.uuid, kind, n)?;
    db::set_step_done(conn, step_id, false, None, None)?;
    println!("Reopened {} {} of task {}.", kind, n, task.id.unwrap_or(0));
    Ok(())
}

/// `sara step remove <id> <N> [--kind acceptance]` — delete a checklist item.
pub fn step_remove(
    conn: &Connection,
    _cfg: &Config,
    id: &str,
    n: usize,
    kind: Option<&str>,
) -> Result<()> {
    let task = db::resolve_task(conn, id)?;
    let kind = kind_arg(kind);
    let steps = db::get_steps(conn, &task.uuid, kind)?;
    let item = steps
        .get(n.saturating_sub(1))
        .ok_or_else(|| anyhow::anyhow!("No {kind} #{n} on this task"))?;
    let text = item.text.clone();
    db::delete_step(conn, item.id)?;
    println!(
        "Removed {} {} of task {}: {}",
        kind,
        n,
        task.id.unwrap_or(0),
        text
    );
    Ok(())
}

/// Add a checklist step (or acceptance criterion) to a task's guide, returning a
/// structured record. Print-free core shared by the CLI `check` command and the
/// MCP `check` tool.
pub fn check_value(
    conn: &Connection,
    id: &str,
    text: &str,
    intent: Option<&str>,
    kind: Option<&str>,
    source: Option<&str>,
    verify: Option<&str>,
) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let kind = kind_arg(kind);
    let source = source.unwrap_or("human");
    let step_id = db::add_step(conn, &task.uuid, text, intent, kind, source, verify)?;
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "kind": kind,
        "text": text,
        "step_id": step_id,
    }))
}

/// `sara verify [--step N] [--run]` — surface/run verification commands.
pub fn verify(
    conn: &Connection,
    _cfg: &Config,
    id: &str,
    step: Option<usize>,
    run: bool,
) -> Result<()> {
    let task = db::resolve_task(conn, id)?;
    let steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;
    let meta = db::get_guide_fields(conn, &task.uuid)?.meta_json;

    let mut cmds: Vec<String> = vec![];

    if let Some(n) = step {
        if let Some(s) = steps.get(n.saturating_sub(1)) {
            if let Some(v) = &s.verify_cmd {
                cmds.push(v.clone());
            } else {
                println!("Step {n} has no verify command.");
            }
        } else {
            anyhow::bail!("No step #{n}");
        }
    } else {
        for s in steps.iter().chain(acceptance.iter()) {
            if let Some(v) = &s.verify_cmd {
                cmds.push(v.clone());
            }
        }
        // Project/task-level test + lint commands from meta_json.
        if let Some(meta) = meta
            .as_deref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        {
            for key in ["test_cmd", "lint_cmd"] {
                if let Some(c) = meta.get(key).and_then(|v| v.as_str()) {
                    cmds.push(c.to_string());
                }
            }
        }
    }

    if !acceptance.is_empty() && step.is_none() {
        println!("Acceptance criteria:");
        for (i, a) in acceptance.iter().enumerate() {
            let mark = if a.done { "[x]" } else { "[ ]" };
            println!("  {} {}. {}", mark, i + 1, a.text);
        }
    }

    if cmds.is_empty() {
        println!("No verification commands found.");
        return Ok(());
    }

    let working_dir = db::get_project(conn, &task.project)
        .ok()
        .flatten()
        .and_then(|p| p.path);

    for cmd in &cmds {
        if run {
            println!("$ {cmd}");
            let mut command = std::process::Command::new("sh");
            command.arg("-c").arg(cmd);
            if let Some(dir) = &working_dir {
                command.current_dir(dir);
            }
            let status = command.status();
            match status {
                Ok(s) if s.success() => println!("  ok: passed"),
                Ok(s) => println!("  exited with {}", s.code().unwrap_or(-1)),
                Err(e) => println!("  failed to run: {e}"),
            }
        } else {
            println!("{cmd}");
        }
    }
    Ok(())
}

/// Read-only structured verification view for the MCP `verify` tool: the
/// verification commands (step + acceptance `verify_cmd`s and project-level
/// test/lint commands) plus the acceptance criteria. Unlike the CLI `verify`,
/// this NEVER executes anything — the agent runs the returned commands itself.
pub fn verify_value(conn: &Connection, id: &str, step: Option<usize>) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;
    let meta = db::get_guide_fields(conn, &task.uuid)?.meta_json;

    let mut cmds: Vec<String> = vec![];
    if let Some(n) = step {
        let s = steps
            .get(n.saturating_sub(1))
            .ok_or_else(|| anyhow::anyhow!("No step #{n}"))?;
        if let Some(v) = &s.verify_cmd {
            cmds.push(v.clone());
        }
    } else {
        for s in steps.iter().chain(acceptance.iter()) {
            if let Some(v) = &s.verify_cmd {
                cmds.push(v.clone());
            }
        }
        if let Some(meta) = meta
            .as_deref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        {
            for key in ["test_cmd", "lint_cmd"] {
                if let Some(c) = meta.get(key).and_then(|v| v.as_str()) {
                    cmds.push(c.to_string());
                }
            }
        }
    }

    let acc: Vec<_> = acceptance
        .iter()
        .enumerate()
        .map(|(i, a)| {
            json!({
                "index": i + 1,
                "text": a.text,
                "done": a.done,
                "verify_cmd": a.verify_cmd,
            })
        })
        .collect();

    Ok(json!({ "task": task.id, "commands": cmds, "acceptance": acc }))
}

/// `sara assignment <id> <text>`
pub fn assignment(conn: &Connection, id: &str, text: &str) -> Result<()> {
    let task = db::resolve_task(conn, id)?;
    db::set_assignment(conn, &task.uuid, text)?;
    println!("Set assignment for task {}.", task.id.unwrap_or(0));
    Ok(())
}

/// `sara rationale <id> <text>`
pub fn rationale(conn: &Connection, id: &str, text: &str) -> Result<()> {
    let task = db::resolve_task(conn, id)?;
    db::set_rationale(conn, &task.uuid, text)?;
    println!("Set rationale for task {}.", task.id.unwrap_or(0));
    Ok(())
}

/// Stamp the guide as validated against the project's current HEAD, returning a
/// structured record. Print-free core shared by the CLI `validate` command and
/// the MCP `validate` tool.
pub fn validate_value(conn: &Connection, id: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let head = project_head(conn, &task.project)
        .ok_or_else(|| anyhow::anyhow!("task's project is not in a git repo"))?;
    db::set_validated(conn, &task.uuid, &head)?;
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "validated_commit": head,
    }))
}

/// `sara validate <id>` — stamp the guide as fresh against current HEAD.
pub fn validate(conn: &Connection, id: &str) -> Result<()> {
    let v = validate_value(conn, id)?;
    println!(
        "Stamped task {} validated @ {}.",
        v["task"].as_i64().unwrap_or(0),
        v["validated_commit"].as_str().unwrap_or_default()
    );
    Ok(())
}

/// Structured form of a task's open feedback. Shared by the `--json` CLI path and
/// the MCP `feedback` tool.
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

/// `sara feedback <id>` — list open human feedback.
pub fn feedback(conn: &Connection, id: &str, as_json: bool) -> Result<()> {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&feedback_value(conn, id)?)?
        );
        return Ok(());
    }

    let task = db::resolve_task(conn, id)?;
    let fb = db::get_open_feedback(conn, &task.uuid)?;

    if fb.is_empty() {
        println!("No open feedback for task {}.", task.id.unwrap_or(0));
        return Ok(());
    }
    for a in &fb {
        let target = match (&a.target_kind, &a.target_id) {
            (Some(k), Some(idv)) => format!(" [{k}:{idv}]"),
            _ => String::new(),
        };
        let flag = if a.request_revision { " ⟳" } else { "" };
        println!("#{}{}{}: {}", a.id, target, flag, a.text);
    }
    Ok(())
}

/// `sara resolve <feedback-id>`
pub fn resolve(conn: &Connection, feedback_id: i64) -> Result<()> {
    if db::resolve_annotation(conn, feedback_id, None)? {
        println!("Resolved feedback #{feedback_id}.");
    } else {
        anyhow::bail!("No feedback with id {feedback_id}");
    }
    Ok(())
}
