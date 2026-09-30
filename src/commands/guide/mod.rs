use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::commands::shared::{
    annotation_target, guard_branch_mutation, insight, item_snippet, memory_handle, print_json,
    project_head,
};
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

mod types;
pub use types::{AcceptanceGate, GateOutput, GateRun};

fn kind_arg(kind: Option<&str>) -> &str {
    match kind {
        Some("acceptance") => db::STEP_KIND_ACCEPTANCE,
        _ => db::STEP_KIND_STEP,
    }
}

const NEXT_MEMORY_LIMIT: usize = 3;

fn relevant_memories(
    conn: &Connection,
    task: &crate::infrastructure::model::Task,
) -> Vec<(String, String)> {
    db::find_similar_strong_memories(conn, &task.description, &task.tags)
        .unwrap_or_default()
        .into_iter()
        .take(NEXT_MEMORY_LIMIT)
        .map(|item| {
            let label = memory_handle(&item);
            let snippet = item_snippet(&item, 160);
            (label, snippet.trim().to_string())
        })
        .collect()
}

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
    if !relevant.is_empty()
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert(
            "relevant_memories".to_string(),
            serde_json::Value::Array(relevant),
        );
    }
    Ok(value)
}

pub fn next(conn: &Connection, _cfg: &Config, id: &str, as_json: bool) -> Result<()> {
    if as_json {
        print_json(&next_value(conn, id)?)?;
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
        println!(
            "\nRelevant memory ({}) — recall before you act:",
            relevant.len()
        );
        for (label, snippet) in &relevant {
            println!("  {label}: {snippet}");
        }
    }
    Ok(())
}

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

pub fn steps(
    conn: &Connection,
    _cfg: &Config,
    id: &str,
    until: Option<usize>,
    as_json: bool,
) -> Result<()> {
    if as_json {
        print_json(&steps_value(conn, id, until)?)?;
        return Ok(());
    }

    let task = db::resolve_task(conn, id)?;
    let mut steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;
    if let Some(n) = until {
        steps.truncate(n);
    }

    if steps.is_empty() && acceptance.is_empty() {
        println!("No steps defined for task {}.", task.id.unwrap_or(0));
        return Ok(());
    }

    if !steps.is_empty() {
        println!("Steps (tick with `step done <id> N`):");
        for (i, s) in steps.iter().enumerate() {
            let mark = if s.done { "[x]" } else { "[ ]" };
            let badge = if s.source == "ai" { " (ai)" } else { "" };
            println!("  {} {}. {}{}", mark, i + 1, s.text, badge);
            if let Some(intent) = &s.intent {
                println!("        {intent}");
            }
        }
    }

    if !acceptance.is_empty() {
        println!("Acceptance criteria (tick with `step done <id> N --kind acceptance`):");
        for (i, a) in acceptance.iter().enumerate() {
            let mark = if a.done { "[x]" } else { "[ ]" };
            let badge = if a.source == "ai" { " (ai)" } else { "" };
            println!("  {} {}. {}{}", mark, i + 1, a.text, badge);
            if let Some(intent) = &a.intent {
                println!("        {intent}");
            }
        }
    }
    Ok(())
}

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

pub fn step_remove_value(
    conn: &Connection,
    id: &str,
    n: usize,
    kind: Option<&str>,
) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let kind = kind_arg(kind);
    let steps = db::get_steps(conn, &task.uuid, kind)?;
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

pub fn step_remove_by_id_value(conn: &Connection, step_id: i64) -> Result<serde_json::Value> {
    let (uuid, kind, index) = db::locate_step(conn, step_id)?;
    step_remove_value(conn, &uuid.to_string(), index, Some(&kind))
}

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
        print_json(&v)?;
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
    let index = db::get_steps(conn, &task.uuid, kind)?.len();
    let mut out = json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "kind": kind,
        "text": text,
        "step_id": step_id,
        "index": index,
    });
    if kind == db::STEP_KIND_ACCEPTANCE && verify.map(str::trim).filter(|v| !v.is_empty()).is_none()
    {
        out["warning"] = json!(
            "acceptance criterion has no verify command — `validate` cannot prove \
             it and will refuse; add one with `--verify \"<cmd>\"`"
        );
    }
    Ok(out)
}

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

pub fn verify_value(conn: &Connection, id: &str, step: Option<usize>) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;
    let meta = db::get_guide_fields(conn, &task.uuid)?.meta_json;

    let mut cmds: Vec<String> = vec![];
    if let Some(n) = step {
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

pub fn assignment_value(conn: &Connection, id: &str, text: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    db::set_assignment(conn, &task.uuid, text)?;
    Ok(json!({ "task": task.id, "uuid": task.uuid.to_string(), "assignment": text }))
}

pub fn assignment(conn: &Connection, id: &str, text: &str) -> Result<()> {
    let v = assignment_value(conn, id, text)?;
    println!(
        "Set assignment for task {}.",
        v["task"].as_i64().unwrap_or(0)
    );
    Ok(())
}

pub fn rationale_value(conn: &Connection, id: &str, text: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    db::set_rationale(conn, &task.uuid, text)?;
    Ok(json!({ "task": task.id, "uuid": task.uuid.to_string(), "rationale": text }))
}

pub fn rationale(conn: &Connection, id: &str, text: &str) -> Result<()> {
    let v = rationale_value(conn, id, text)?;
    println!(
        "Set rationale for task {}.",
        v["task"].as_i64().unwrap_or(0)
    );
    Ok(())
}

fn tail_limited(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_string();
    }
    let skip = total - max_chars;
    let start = s
        .char_indices()
        .nth(skip)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    format!("…[{skip} chars truncated]\n{}", &s[start..])
}

pub fn run_acceptance_gate(
    conn: &Connection,
    task_id_or_uuid: &str,
    mode: GateOutput,
    fresh: bool,
) -> Result<AcceptanceGate> {
    let task = db::resolve_task(conn, task_id_or_uuid)?;
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;
    let working_dir = db::get_project(conn, &task.project)
        .ok()
        .flatten()
        .and_then(|p| p.path);
    let commit = project_head(conn, &task.project);

    let tree_clean = !fresh
        && working_dir
            .as_deref()
            .and_then(|d| crate::infrastructure::git::is_clean(std::path::Path::new(d)))
            .unwrap_or(false);
    let cache_ok = !fresh && tree_clean && commit.is_some();

    let mut gate = AcceptanceGate {
        total: acceptance.len(),
        ran: 0,
        passed: 0,
        cached: 0,
        failures: Vec::new(),
        missing_verify: Vec::new(),
        transcript: Vec::new(),
    };

    let mut ran_cmds: std::collections::HashMap<String, (bool, Option<i32>, String)> =
        std::collections::HashMap::new();

    for s in &acceptance {
        let Some(cmd) = &s.verify_cmd else {
            gate.missing_verify.push(s.text.clone());
            continue;
        };

        if cache_ok && s.done && s.done_commit.as_deref() == commit.as_deref() {
            gate.cached += 1;
            mode.say(format!("$ {cmd}"));
            mode.say(format!(
                "  ⏭ cached — already proven at {} (clean tree), skipped \"{}\"",
                commit.as_deref().unwrap_or("?"),
                s.text
            ));
            gate.transcript.push(GateRun {
                text: s.text.clone(),
                cmd: cmd.clone(),
                passed: true,
                exit_code: Some(0),
                output: format!("cached: proven at {}", commit.as_deref().unwrap_or("?")),
            });
            continue;
        }

        if let Some((passed, code, out)) = ran_cmds.get(cmd).cloned() {
            if passed {
                let note = format!("verify passed: {cmd}");
                db::set_step_done(conn, s.id, true, Some(&note), commit.as_deref())?;
                gate.passed += 1;
                mode.say(format!(
                    "  ↻ same command already run — reusing pass for \"{}\"",
                    s.text
                ));
            } else {
                gate.failures.push(s.text.clone());
                mode.say(format!(
                    "  ↻ same command already run — reusing failure for \"{}\"",
                    s.text
                ));
            }
            gate.transcript.push(GateRun {
                text: s.text.clone(),
                cmd: cmd.clone(),
                passed,
                exit_code: code,
                output: out,
            });
            continue;
        }

        gate.ran += 1;
        mode.say(format!("$ {cmd}"));
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(cmd);
        if let Some(dir) = &working_dir {
            command.current_dir(dir);
        }

        let outcome = match mode {
            GateOutput::Stream => command.status().map(|st| (st, String::new())),
            GateOutput::Capture => command.output().map(|o| {
                let mut buf = String::from_utf8_lossy(&o.stdout).into_owned();
                let err = String::from_utf8_lossy(&o.stderr);
                if !err.is_empty() {
                    if !buf.is_empty() && !buf.ends_with('\n') {
                        buf.push('\n');
                    }
                    buf.push_str(&err);
                }
                (o.status, tail_limited(&buf, 4000))
            }),
        };

        match outcome {
            Ok((st, out)) if st.success() => {
                let note = format!("verify passed: {cmd}");
                db::set_step_done(conn, s.id, true, Some(&note), commit.as_deref())?;
                gate.passed += 1;
                mode.say(format!("  ✓ passed — ticked \"{}\"", s.text));
                ran_cmds.insert(cmd.clone(), (true, st.code(), out.clone()));
                gate.transcript.push(GateRun {
                    text: s.text.clone(),
                    cmd: cmd.clone(),
                    passed: true,
                    exit_code: st.code(),
                    output: out,
                });
            }
            Ok((st, out)) => {
                let code = st.code().unwrap_or(-1);
                mode.say(format!("  ✗ exit {code} — \"{}\"", s.text));
                gate.failures.push(s.text.clone());
                ran_cmds.insert(cmd.clone(), (false, st.code(), out.clone()));
                gate.transcript.push(GateRun {
                    text: s.text.clone(),
                    cmd: cmd.clone(),
                    passed: false,
                    exit_code: st.code(),
                    output: out,
                });
            }
            Err(e) => {
                mode.say(format!("  ✗ failed to run ({e}) — \"{}\"", s.text));
                gate.failures.push(s.text.clone());
                let msg = format!("failed to run: {e}");
                ran_cmds.insert(cmd.clone(), (false, None, msg.clone()));
                gate.transcript.push(GateRun {
                    text: s.text.clone(),
                    cmd: cmd.clone(),
                    passed: false,
                    exit_code: None,
                    output: msg,
                });
            }
        }
    }
    Ok(gate)
}

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

#[cfg(test)]
#[path = "../../../tests/unit/commands/guide/mod.rs"]
mod tests;
