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

/// Refuse a finalizing mutation targeted by a **recyclable numeric display id**
/// when the resolved task is tied to a git branch other than the one currently
/// checked out in its project — the signature of "display-id recompaction moved
/// id N onto a different task than you meant" (the concurrent-agent hazard).
///
/// Positive-evidence only: stays silent when the input was a uuid (stable), the
/// task has no branch tie, the branch can't be determined (detached HEAD / no
/// repo), or `force` is set — so legitimate single-branch flows never trip.
pub fn guard_branch_mutation(
    conn: &Connection,
    id_input: &str,
    task: &crate::infrastructure::model::Task,
    force: bool,
) -> Result<()> {
    if force || id_input.parse::<i64>().is_err() {
        return Ok(()); // uuid inputs are stable; --force opts out.
    }
    let Some(rec) = db::get_task_branch(conn, &task.uuid) else {
        return Ok(());
    };
    let Some(path) = db::get_project(conn, &task.project)
        .ok()
        .flatten()
        .and_then(|p| p.path)
    else {
        return Ok(());
    };
    let Some(current) = crate::infrastructure::git::current_branch(std::path::Path::new(&path))
    else {
        return Ok(());
    };
    if current != rec.branch {
        anyhow::bail!(
            "refusing to act on task {} by display id — it is tied to branch '{}' but the \
             project is currently on '{}'. Recycled display ids can point at a different task \
             after recompaction. Re-run with the stable uuid `{}` (or pass --force).",
            task.id.unwrap_or(0),
            rec.branch,
            current,
            &task.uuid.to_string()[..8]
        );
    }
    Ok(())
}

fn kind_arg(kind: Option<&str>) -> &str {
    match kind {
        Some("acceptance") => db::STEP_KIND_ACCEPTANCE,
        _ => db::STEP_KIND_STEP,
    }
}

/// Max memories surfaced on the execution cursor. Tighter than the task-start
/// guide (5): `sara next` is hit on every step, so it must stay a glance, not a
/// wall.
const NEXT_MEMORY_LIMIT: usize = 3;

/// Strong memories relevant to a task's description + tags, as `(label, snippet)`
/// pairs — the same signal `sara info`/`guide` surfaces at task-start, brought
/// into the per-step execution cursor so the agent never works memory-blind.
/// Empty when nothing Strong matches (the caller then omits the block entirely).
fn relevant_memories(conn: &Connection, task: &crate::infrastructure::model::Task) -> Vec<(String, String)> {
    db::find_similar_strong_memories(conn, &task.description, &task.tags)
        .unwrap_or_default()
        .into_iter()
        .take(NEXT_MEMORY_LIMIT)
        .map(|item| {
            let label = format!("m{}", item.display_id.unwrap_or(0));
            let snippet: String = item
                .summary
                .clone()
                .unwrap_or_else(|| item.body.clone())
                .chars()
                .take(160)
                .collect();
            (label, snippet.trim().to_string())
        })
        .collect()
}

/// Structured form of the execution cursor (first not-done step). Shared by the
/// `--json` CLI path and the MCP `next` tool so there is a single serializer.
pub fn next_value(conn: &Connection, id: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    let next = steps.iter().enumerate().find(|(_, s)| !s.done);
    let relevant: Vec<serde_json::Value> = relevant_memories(conn, &task)
        .into_iter()
        .map(|(label, snippet)| json!({ "label": label, "snippet": snippet }))
        .collect();
    let mut value = match next {
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
    };
    if !relevant.is_empty() {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("relevant_memories".to_string(), serde_json::Value::Array(relevant));
        }
    }
    Ok(value)
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

    let relevant = relevant_memories(conn, &task);
    if !relevant.is_empty() {
        println!("\nRelevant memory ({}) — recall before you act:", relevant.len());
        for (label, snippet) in &relevant {
            println!("  {label}: {snippet}");
        }
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
    // First recorded work auto-transitions the task to active (Feature: status
    // should reflect reality without a separate `sara start`).
    let activated = db::ensure_started(conn, &task.uuid)?;
    // Resurface prior findings semantically close to this result — reconnect the
    // agent to what it already concluded before it moves on.
    let related = match result {
        Some(r) if !r.trim().is_empty() => {
            crate::commands::insight::related_findings(conn, &task.uuid, r, None)
        }
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
        "related_findings": crate::commands::insight::related_findings_json(&related),
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
    as_json: bool,
) -> Result<()> {
    let v = step_done_value(conn, id, n, result, kind)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&v)?);
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
    if let Some(related) = v.get("related_findings").and_then(|r| r.as_array())
        && !related.is_empty()
    {
        eprintln!("⟳ reconsider — related prior finding(s) on this task:");
        for r in related {
            eprintln!(
                "    (~{:.2}) #{}: {}",
                r.get("cosine").and_then(|c| c.as_f64()).unwrap_or(0.0),
                r.get("annotation_id").and_then(|i| i.as_i64()).unwrap_or(0),
                r.get("text").and_then(|t| t.as_str()).unwrap_or("")
            );
        }
    }
    Ok(())
}

/// Reopen step `n` and return a structured record. Print-free core shared by the
/// CLI `step undone` command and the MCP `step_undone` tool.
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

/// `sara step undone <id> <n>` — reopen a step.
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
        println!("{}", serde_json::to_string_pretty(&v)?);
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

/// Delete checklist item `n` and return a structured record. Print-free core
/// shared by the CLI `step remove` command and the MCP `step_remove` tool.
pub fn step_remove_value(
    conn: &Connection,
    id: &str,
    n: usize,
    kind: Option<&str>,
) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let kind = kind_arg(kind);
    let steps = db::get_steps(conn, &task.uuid, kind)?;
    // Indices are 1-based: reject 0 rather than letting it fall through to item 1.
    let idx = n
        .checked_sub(1)
        .ok_or_else(|| anyhow::anyhow!("{kind} index is 1-based; got 0"))?;
    let item = steps
        .get(idx)
        .ok_or_else(|| anyhow::anyhow!("No {kind} #{n} on this task"))?;
    let text = item.text.clone();
    db::delete_step(conn, item.id)?;
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "kind": kind,
        "index": n,
        "removed": text,
    }))
}

/// `sara step remove <id> <N> [--kind acceptance]` — delete a checklist item.
pub fn step_remove(
    conn: &Connection,
    _cfg: &Config,
    id: &str,
    n: usize,
    kind: Option<&str>,
    as_json: bool,
) -> Result<()> {
    let v = step_remove_value(conn, id, n, kind)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    println!(
        "Removed {} {} of task {}: {}",
        v["kind"].as_str().unwrap_or("step"),
        n,
        v["task"].as_i64().unwrap_or(0),
        v["removed"].as_str().unwrap_or_default()
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

/// `sara verify [--step N] [--run] [--tick-on-pass]` — surface/run verification
/// commands. With `--tick-on-pass`, each step / acceptance criterion that carries
/// a stored verify command is executed and marked done **only** when it exits 0,
/// and the pass/fail outcome is recorded as that item's execution result —
/// collapsing "run the check" and "tick the box" into a single call.
pub fn verify(
    conn: &Connection,
    _cfg: &Config,
    id: &str,
    step: Option<usize>,
    run: bool,
    tick_on_pass: bool,
) -> Result<()> {
    let task = db::resolve_task(conn, id)?;
    let steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;
    let meta = db::get_guide_fields(conn, &task.uuid)?.meta_json;

    let working_dir = db::get_project(conn, &task.project)
        .ok()
        .flatten()
        .and_then(|p| p.path);

    // ── tick-on-pass: run each criterion's own verify_cmd, tick on exit 0 ──
    if tick_on_pass {
        let commit = project_head(conn, &task.project);
        let targets: Vec<&_> = if let Some(n) = step {
            let idx = n
                .checked_sub(1)
                .ok_or_else(|| anyhow::anyhow!("step index is 1-based; got 0"))?;
            let s = steps
                .get(idx)
                .ok_or_else(|| anyhow::anyhow!("No step #{n}"))?;
            vec![s]
        } else {
            steps.iter().chain(acceptance.iter()).collect()
        };

        let (mut ran, mut passed) = (0usize, 0usize);
        let mut started_noted = false;
        for s in targets {
            let Some(cmd) = &s.verify_cmd else { continue };
            ran += 1;
            // First executed check auto-transitions the task to active.
            if !started_noted {
                if db::ensure_started(conn, &task.uuid)? {
                    println!("Task {} is now active.", task.id.unwrap_or(0));
                }
                started_noted = true;
            }
            println!("$ {cmd}");
            let mut command = std::process::Command::new("sh");
            command.arg("-c").arg(cmd);
            if let Some(dir) = &working_dir {
                command.current_dir(dir);
            }
            match command.status() {
                Ok(st) if st.success() => {
                    let note = format!("verify passed: {cmd}");
                    db::set_step_done(conn, s.id, true, Some(&note), commit.as_deref())?;
                    passed += 1;
                    println!("  ✓ passed — ticked \"{}\"", s.text);
                }
                Ok(st) => {
                    let code = st.code().unwrap_or(-1);
                    println!("  ✗ exit {code} — left unticked: \"{}\"", s.text);
                }
                Err(e) => {
                    println!("  ✗ failed to run ({e}) — left unticked: \"{}\"", s.text);
                }
            }
        }
        if ran == 0 {
            println!("No steps or acceptance criteria have a verify command to run.");
        } else {
            println!("Ticked {passed}/{ran} on pass.");
        }
        return Ok(());
    }

    let mut cmds: Vec<String> = vec![];

    if let Some(n) = step {
        let idx = n
            .checked_sub(1)
            .ok_or_else(|| anyhow::anyhow!("step index is 1-based; got 0"))?;
        if let Some(s) = steps.get(idx) {
            if let Some(v) = &s.verify_cmd {
                cmds.push(v.clone());
            } else {
                println!("Step {n} has no verify command.");
            }
        } else {
            anyhow::bail!("No step #{n}");
        }
    } else {
        // Project-level setup/test/lint commands (from `sara init`), same
        // ones shown in `sara info`'s Verification section. `run_cmd` is
        // deliberately excluded here — it's typically a long-lived server
        // process, not something safe to execute as a verification step.
        let pc = db::get_project_commands(conn, &task.project)?;
        for c in [&pc.setup_cmd, &pc.test_cmd, &pc.lint_cmd]
            .into_iter()
            .flatten()
        {
            cmds.push(c.clone());
        }
        for s in steps.iter().chain(acceptance.iter()) {
            if let Some(v) = &s.verify_cmd {
                cmds.push(v.clone());
            }
        }
        // Task-level test + lint command overrides from meta_json.
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

    // Actually running a verification transitions the task to active.
    if run && db::ensure_started(conn, &task.uuid)? {
        println!("Task {} is now active.", task.id.unwrap_or(0));
    }

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
        // Indices are 1-based: reject 0 rather than silently returning step 1.
        let idx = n
            .checked_sub(1)
            .ok_or_else(|| anyhow::anyhow!("step index is 1-based; got 0"))?;
        let s = steps
            .get(idx)
            .ok_or_else(|| anyhow::anyhow!("No step #{n}"))?;
        if let Some(v) = &s.verify_cmd {
            cmds.push(v.clone());
        }
    } else {
        let pc = db::get_project_commands(conn, &task.project)?;
        for c in [&pc.setup_cmd, &pc.test_cmd, &pc.lint_cmd]
            .into_iter()
            .flatten()
        {
            cmds.push(c.clone());
        }
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

/// Set a task's assignment text; print-free core shared by the CLI and MCP tool.
pub fn assignment_value(conn: &Connection, id: &str, text: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    db::set_assignment(conn, &task.uuid, text)?;
    Ok(json!({ "task": task.id, "uuid": task.uuid.to_string(), "assignment": text }))
}

/// `sara assignment <id> <text>`
pub fn assignment(conn: &Connection, id: &str, text: &str) -> Result<()> {
    let v = assignment_value(conn, id, text)?;
    println!(
        "Set assignment for task {}.",
        v["task"].as_i64().unwrap_or(0)
    );
    Ok(())
}

/// Set a task's rationale text; print-free core shared by the CLI and MCP tool.
pub fn rationale_value(conn: &Connection, id: &str, text: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    db::set_rationale(conn, &task.uuid, text)?;
    Ok(json!({ "task": task.id, "uuid": task.uuid.to_string(), "rationale": text }))
}

/// `sara rationale <id> <text>`
pub fn rationale(conn: &Connection, id: &str, text: &str) -> Result<()> {
    let v = rationale_value(conn, id, text)?;
    println!(
        "Set rationale for task {}.",
        v["task"].as_i64().unwrap_or(0)
    );
    Ok(())
}

/// Stamp the guide as validated against the project's current HEAD, returning a
/// Outcome of running a task's acceptance criteria as a pass/fail gate.
pub struct AcceptanceGate {
    /// Total acceptance criteria on the task.
    pub total: usize,
    /// Criteria whose stored `verify_cmd` was executed.
    pub ran: usize,
    /// Criteria that ran and exited 0 (ticked as a side effect).
    pub passed: usize,
    /// Text of criteria that ran but exited non-zero.
    pub failures: Vec<String>,
    /// Text of criteria that carry NO `verify_cmd` — unprovable, so they block.
    pub missing_verify: Vec<String>,
}

impl AcceptanceGate {
    /// The gate is green only when there is at least one acceptance criterion,
    /// every one carries a verify command, and every command passed. "No
    /// acceptance criteria" is a red gate: a task with no definition of done
    /// cannot be proven complete.
    pub fn is_green(&self) -> bool {
        self.total > 0 && self.failures.is_empty() && self.missing_verify.is_empty()
    }

    /// A human/agent-readable reason the gate is red.
    pub fn reason(&self) -> String {
        if self.total == 0 {
            return "no acceptance criteria to prove — add one with `sara check <id> \"…\" --kind acceptance --verify \"<cmd>\"`".to_string();
        }
        let mut parts = Vec::new();
        if !self.missing_verify.is_empty() {
            parts.push(format!(
                "{} acceptance criterion/criteria have no verify command: {}",
                self.missing_verify.len(),
                self.missing_verify.join("; ")
            ));
        }
        if !self.failures.is_empty() {
            parts.push(format!(
                "{} acceptance verify command(s) failed: {}",
                self.failures.len(),
                self.failures.join("; ")
            ));
        }
        parts.join(" | ")
    }
}

/// Run every acceptance criterion's `verify_cmd`, ticking those that exit 0, and
/// report the aggregate outcome. This is the engine behind the fail-closed
/// `validate`: "green" can only be recorded when a real command proved it, never
/// asserted in prose. Criteria with no verify command are reported as blocking
/// (`missing_verify`) rather than silently passed.
pub fn run_acceptance_gate(conn: &Connection, task_id_or_uuid: &str) -> Result<AcceptanceGate> {
    let task = db::resolve_task(conn, task_id_or_uuid)?;
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;
    let working_dir = db::get_project(conn, &task.project)
        .ok()
        .flatten()
        .and_then(|p| p.path);
    let commit = project_head(conn, &task.project);

    let mut gate = AcceptanceGate {
        total: acceptance.len(),
        ran: 0,
        passed: 0,
        failures: Vec::new(),
        missing_verify: Vec::new(),
    };

    for s in &acceptance {
        let Some(cmd) = &s.verify_cmd else {
            gate.missing_verify.push(s.text.clone());
            continue;
        };
        gate.ran += 1;
        println!("$ {cmd}");
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(cmd);
        if let Some(dir) = &working_dir {
            command.current_dir(dir);
        }
        match command.status() {
            Ok(st) if st.success() => {
                let note = format!("verify passed: {cmd}");
                db::set_step_done(conn, s.id, true, Some(&note), commit.as_deref())?;
                gate.passed += 1;
                println!("  ✓ passed — ticked \"{}\"", s.text);
            }
            Ok(st) => {
                let code = st.code().unwrap_or(-1);
                println!("  ✗ exit {code} — \"{}\"", s.text);
                gate.failures.push(s.text.clone());
            }
            Err(e) => {
                println!("  ✗ failed to run ({e}) — \"{}\"", s.text);
                gate.failures.push(s.text.clone());
            }
        }
    }
    Ok(gate)
}

/// structured record. Print-free core shared by the CLI `validate` command and
/// the MCP `validate` tool.
///
/// Fail-closed: unless `skip_gate` is set, every acceptance criterion's
/// `verify_cmd` is executed and must exit 0 (and every criterion must carry a
/// command) before the guide is stamped. This makes "validated" mean *proven
/// green by a command*, never merely asserted.
pub fn validate_value(conn: &Connection, id: &str, skip_gate: bool) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    guard_branch_mutation(conn, id, &task, false)?;
    let head = project_head(conn, &task.project)
        .ok_or_else(|| anyhow::anyhow!("task's project is not in a git repo"))?;

    if !skip_gate {
        let gate = run_acceptance_gate(conn, id)?;
        if !gate.is_green() {
            anyhow::bail!(
                "validate refused — acceptance gate is red: {}. \
                 Fix and re-run, or `validate --no-run` to stamp without proof (discouraged).",
                gate.reason()
            );
        }
    }

    db::set_validated(conn, &task.uuid, &head)?;
    Ok(json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "validated_commit": head,
        "gate_skipped": skip_gate,
    }))
}

/// `sara validate <id>` — prove every acceptance criterion green, then stamp the
/// guide as fresh against current HEAD. With `no_run`, stamps without running
/// the gate (escape hatch for environments where the checks cannot run locally).
pub fn validate(conn: &Connection, id: &str, no_run: bool) -> Result<()> {
    if no_run {
        eprintln!(
            "⚠ validate --no-run: stamping WITHOUT running the acceptance gate — \
             'validated' will not be backed by a passing command."
        );
    }
    let v = validate_value(conn, id, no_run)?;
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

/// Resolve a feedback (annotation) item by its id; print-free core shared by the
/// CLI `resolve` command and the MCP `resolve` tool. Errors if no such feedback.
/// `run_id` optionally links the resolution to the AI run (see `record_run_value`)
/// that addressed it, so the provenance is traceable later.
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

/// `sara resolve <feedback-id> [--run <run-id>]`
pub fn resolve(conn: &Connection, feedback_id: i64, run_id: Option<i64>) -> Result<()> {
    resolve_value(conn, feedback_id, run_id)?;
    println!("Resolved feedback #{feedback_id}.");
    Ok(())
}

/// Record one AI/LLM interaction against a task (an audit-trail entry shown in
/// `sara info`'s "AI activity" section); print-free core shared by the CLI
/// `record-run` command and the MCP `record_run` tool. Returns the new run id
/// so it can be cited later via `resolve --run <run-id>`.
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

/// `sara record-run <id> --kind <KIND> [--model] [--provider] [--prompt] [--response]`
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::config::Config;
    use crate::infrastructure::model::Task;

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn next_surfaces_a_strong_relevant_memory() {
        use crate::infrastructure::model::Item;
        let conn = db::open_in_memory_for_test();

        // A completed source task lifts memories derived from it to Strong (2.0).
        let mut src = Task::new("source work".into(), "proj".into());
        src.status = crate::infrastructure::model::Status::Completed;
        db::insert_task(&conn, &mut src).unwrap();

        // A Strong memory tagged `dependabot`, derived from the completed task.
        let mut mem = Item::new_memory(
            "dependabot bump restore pattern".into(),
            "dependabot bump broke restore; align versions".into(),
            Some(src.uuid),
        );
        mem.tags = vec!["dependabot".into()];
        mem.path = Some(String::new());
        db::insert_item(&conn, &mut mem).unwrap();
        db::set_item_tags(&conn, &mem.uuid, &["dependabot".into()]).unwrap();

        // The current task shares the tag, so the memory is relevant to it.
        let mut task = Task::new("do the dependabot bump".into(), "proj".into());
        task.tags = vec!["dependabot".into()];
        db::insert_task(&conn, &mut task).unwrap();
        db::add_step(&conn, &task.uuid, "step one", None, db::STEP_KIND_STEP, "human", None)
            .unwrap();

        let v = next_value(&conn, &task.uuid.to_string()).unwrap();
        let mems = v["relevant_memories"]
            .as_array()
            .expect("relevant_memories present when a Strong memory matches");
        assert_eq!(mems.len(), 1);
        assert!(mems[0]["label"].as_str().unwrap().starts_with('m'));
        assert!(!mems[0]["snippet"].as_str().unwrap().is_empty());
    }

    #[test]
    fn next_omits_the_block_when_no_memory_matches() {
        let conn = db::open_in_memory_for_test();
        let mut task = Task::new("unrelated task".into(), "proj".into());
        task.tags = vec!["nothing-matches-this".into()];
        db::insert_task(&conn, &mut task).unwrap();
        db::add_step(&conn, &task.uuid, "step one", None, db::STEP_KIND_STEP, "human", None)
            .unwrap();

        let v = next_value(&conn, &task.uuid.to_string()).unwrap();
        assert!(
            v.get("relevant_memories").is_none(),
            "no relevant_memories key when nothing Strong matches"
        );
    }

    #[test]
    fn tick_on_pass_ticks_passing_criteria_and_activates_task() {
        let conn = db::open_in_memory_for_test();
        let mut task = Task::new("demo".into(), "proj".into());
        db::insert_task(&conn, &mut task).unwrap();
        // Two acceptance criteria: one whose verify passes, one that fails.
        db::add_step(
            &conn,
            &task.uuid,
            "passes",
            None,
            db::STEP_KIND_ACCEPTANCE,
            "human",
            Some("true"),
        )
        .unwrap();
        db::add_step(
            &conn,
            &task.uuid,
            "fails",
            None,
            db::STEP_KIND_ACCEPTANCE,
            "human",
            Some("false"),
        )
        .unwrap();

        let id = task.uuid.to_string();
        verify(&conn, &cfg(), &id, None, false, true).unwrap();

        let acc = db::get_steps(&conn, &task.uuid, db::STEP_KIND_ACCEPTANCE).unwrap();
        assert!(acc[0].done, "criterion with a passing verify_cmd is ticked");
        assert!(acc[0].result.as_deref().unwrap_or("").contains("passed"));
        assert!(!acc[1].done, "criterion whose verify_cmd fails stays unticked");

        // Running a check auto-transitions the task to active.
        let reloaded = db::get_task_by_uuid_prefix(&conn, &id).unwrap().unwrap();
        assert!(reloaded.started_at.is_some());
    }

    fn task_with_acceptance(conn: &Connection, verify: Option<&str>) -> Task {
        let mut task = Task::new("gate demo".into(), "proj".into());
        db::insert_task(conn, &mut task).unwrap();
        db::add_step(
            conn,
            &task.uuid,
            "criterion",
            None,
            db::STEP_KIND_ACCEPTANCE,
            "human",
            verify,
        )
        .unwrap();
        task
    }

    #[test]
    fn gate_is_green_only_when_every_criterion_has_a_passing_verify() {
        let conn = db::open_in_memory_for_test();
        let task = task_with_acceptance(&conn, Some("true"));
        let gate = run_acceptance_gate(&conn, &task.uuid.to_string()).unwrap();
        assert!(gate.is_green(), "one criterion, verify passes → green");
        assert_eq!(gate.passed, 1);
    }

    #[test]
    fn gate_red_when_verify_command_fails() {
        let conn = db::open_in_memory_for_test();
        let task = task_with_acceptance(&conn, Some("false"));
        let gate = run_acceptance_gate(&conn, &task.uuid.to_string()).unwrap();
        assert!(!gate.is_green(), "failing verify → red");
        assert_eq!(gate.failures.len(), 1);
    }

    #[test]
    fn gate_red_when_a_criterion_has_no_verify_command() {
        let conn = db::open_in_memory_for_test();
        let task = task_with_acceptance(&conn, None);
        let gate = run_acceptance_gate(&conn, &task.uuid.to_string()).unwrap();
        assert!(!gate.is_green(), "unprovable criterion → red");
        assert_eq!(gate.missing_verify.len(), 1);
    }

    #[test]
    fn gate_red_when_no_acceptance_criteria_exist() {
        let conn = db::open_in_memory_for_test();
        let mut task = Task::new("no criteria".into(), "proj".into());
        db::insert_task(&conn, &mut task).unwrap();
        let gate = run_acceptance_gate(&conn, &task.uuid.to_string()).unwrap();
        assert!(!gate.is_green(), "no definition of done → red");
        assert_eq!(gate.total, 0);
    }

    #[test]
    fn step_done_value_reports_activation_on_first_work() {
        let conn = db::open_in_memory_for_test();
        let mut task = Task::new("demo".into(), "proj".into());
        db::insert_task(&conn, &mut task).unwrap();
        db::add_step(
            &conn,
            &task.uuid,
            "do it",
            None,
            db::STEP_KIND_STEP,
            "human",
            None,
        )
        .unwrap();

        let id = task.uuid.to_string();
        let v = step_done_value(&conn, &id, 1, None, None).unwrap();
        assert_eq!(v["activated"], true, "first recorded work activates the task");

        // A second step-done on an already-active task does not re-activate.
        db::add_step(
            &conn,
            &task.uuid,
            "again",
            None,
            db::STEP_KIND_STEP,
            "human",
            None,
        )
        .unwrap();
        let v2 = step_done_value(&conn, &id, 2, None, None).unwrap();
        assert_eq!(v2["activated"], false);
    }
}
