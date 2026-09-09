//! `sara begin` — the single entry point for starting a task.
//!
//! One call founds the task AND surfaces the memory that bears on it, so the
//! very first sara action already yields a real task with related prior art
//! bound to it. It is a THIN composition of the existing value functions
//! (`add`, `assignment`, `rationale`, `check`, `recall`, `annotate`, `next`) —
//! it introduces no new storage and stays deliberately skill/tooling-agnostic:
//! it speaks only of tasks, acceptance criteria and memories, never of any
//! particular workflow, rite, or agent methodology.
//!
//! Sequence:
//!   1. create the task from the description (tags / files / priority),
//!   2. set its assignment (the originating request) and, if given, its why,
//!   3. register an acceptance criterion — OPTIONAL: warn but proceed if none,
//!   4. recall prior art (query derived from the description+tags+files unless
//!      `--query` overrides it), query-driven so global prior art still surfaces,
//!   5. auto-annotate a compact `finding` naming the recalled memories, so the
//!      lookup is bound to the work instead of evaporating,
//!   6. print the task, its criteria, the recall hits, and the next cursor.

use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::commands;
use crate::infrastructure::config::Config;

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
    query: Option<&str>,
    limit: i64,
) -> Result<Value> {
    if description.trim().is_empty() {
        anyhow::bail!("Task description cannot be empty");
    }

    let mut warnings: Vec<String> = Vec::new();

    // 1. Create the task.
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

    // 2. Assignment — the originating request. Defaults to the description so
    //    the task always records what was actually asked for.
    let assignment_text = assignment
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| description.trim());
    commands::guide::assignment_value(conn, &id, assignment_text)?;

    // 3. Rationale (why this task exists), when supplied.
    let rationale_text = rationale.map(str::trim).filter(|s| !s.is_empty());
    if let Some(why) = rationale_text {
        commands::guide::rationale_value(conn, &id, why)?;
    }

    // 4. Acceptance criterion — OPTIONAL. A task without a definition of done is
    //    allowed to proceed, but we warn so it is a deliberate choice, not a
    //    silent gap.
    let acceptance = match check.map(str::trim).filter(|s| !s.is_empty()) {
        Some(text) => Some(commands::guide::check_value(
            conn,
            &id,
            text,
            None,
            Some("acceptance"),
            Some("agent"),
            verify,
        )?),
        None => {
            warnings.push(
                "no acceptance criterion — define done with \
                 `sara check <id> \"…\" --kind acceptance --verify \"<cmd>\"`"
                    .to_string(),
            );
            None
        }
    };

    // 5. Recall prior art. The query is derived from the description, tags, and
    //    file names unless `--query` overrides it. It is deliberately
    //    query-driven and NOT hard-scoped by project/tag/file: most prior art
    //    lives in global or differently-tagged memories, so an exclusive filter
    //    would surface nothing. The tag/file words instead enrich the query so
    //    relevant memories rank up without being filtered out.
    let derived = derive_query(description, tags, files);
    let recall_query = query
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or(derived);
    let recall_started = std::time::Instant::now();
    let recall =
        commands::recall::recall_value(conn, cfg, &recall_query, &[], &[], &[], limit, false)?;
    let recall_ms = recall_started.elapsed().as_millis() as u64;
    let labels = recall_labels(&recall, id_num);

    // 6. Bind the recall onto the task as a compact finding, so the lookup is
    //    part of the record instead of a throwaway console read.
    let finding = if labels.is_empty() {
        warnings.push("recall surfaced no prior art for this task".to_string());
        Value::Null
    } else {
        let text = format!("recall at task start matched {}", labels.join(", "));
        commands::annotate::annotate_value(
            conn,
            &id,
            std::slice::from_ref(&text),
            Some("finding"),
            Some("sara"),
            None,
            false,
        )?;
        Value::String(text)
    };

    // 7. The execution cursor — where the work goes next.
    let next = commands::guide::next_value(conn, &id)?;

    Ok(json!({
        "task": id_num,
        "uuid": created["uuid"],
        "project": created["project"],
        "description": description.trim(),
        "assignment": assignment_text,
        "rationale": rationale_text,
        "acceptance": acceptance,
        "recall": {
            "query": recall_query,
            "matched": labels,
            "keyword": recall["keyword"],
            "associative": recall["associative"],
            "duration_ms": recall_ms,
        },
        "finding": finding,
        "next": next,
        "warnings": warnings,
    }))
}

/// `sara begin` — CLI entry: found the task, recall prior art, print a summary
/// (or the raw JSON with `--json`).
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
    query: Option<&str>,
    limit: i64,
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
        query,
        limit,
    )?;

    // `begin` folds a `recall` inside it, so that lookup never reaches the CLI
    // dispatcher's own telemetry. Emit a distinct, nested event for it so the
    // folded recall is visible in the usage log rather than silently absorbed.
    crate::infrastructure::telemetry::capture(
        cfg,
        crate::infrastructure::telemetry::Source::Cli,
        "recall",
        &["via_begin".to_string()],
        v["recall"]["duration_ms"].as_u64().unwrap_or(0),
        &Ok::<(), anyhow::Error>(()),
        None,
    );

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

    let matched = v["recall"]["matched"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if matched.is_empty() {
        println!("  recall: no prior art");
    } else {
        let names: Vec<&str> = matched.iter().filter_map(|m| m.as_str()).collect();
        println!(
            "  recall: matched {} (bound as a finding)",
            names.join(", ")
        );
    }

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

/// Derive the recall query from the task description plus any tags and the base
/// names of touched files — the plain, meaning-bearing words an agent would
/// otherwise re-type. Deliberately naive: recall's own matching does the heavy
/// lifting; this only needs to seed it. Folding tags/files into the *query*
/// (rather than passing them as exclusive filters) lets related memories rank
/// up without excluding the global prior art most learnings live in.
fn derive_query(description: &str, tags: &[String], files: &[String]) -> String {
    let mut parts: Vec<String> = vec![description.trim().to_string()];
    for t in tags {
        let t = t.trim();
        if !t.is_empty() {
            parts.push(t.to_string());
        }
    }
    for f in files {
        // Just the base name — a full path adds noise, not signal, to an FTS query.
        let base = f.trim().rsplit(['/', '\\']).next().unwrap_or("").trim();
        if !base.is_empty() {
            parts.push(base.to_string());
        }
    }
    parts.join(" ")
}

/// Collect the labels of the recalled memories (e.g. `m517`) and related tasks
/// from both the keyword and associative bands, de-duplicated and capped so the
/// finding stays a single compact line. The just-created task (`self_task`) is
/// excluded — recall will match it by its own fresh description, and naming it
/// as its own prior art is pure noise.
fn recall_labels(recall: &Value, self_task: i64) -> Vec<String> {
    const MAX: usize = 6;
    let self_label = format!("task {self_task}");
    let mut seen: Vec<String> = Vec::new();
    for band in ["keyword", "associative"] {
        if let Some(hits) = recall[band].as_array() {
            for h in hits {
                if let Some(label) = h["label"].as_str() {
                    let label = label.to_string();
                    if label == self_label || seen.contains(&label) {
                        continue;
                    }
                    seen.push(label);
                    if seen.len() >= MAX {
                        return seen;
                    }
                }
            }
        }
    }
    seen
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
            "Fix the failing build",
            &["ci".to_string()],
            &[],
            Some("proj"),
            None,
            None,
            Some("the restore is red on NU1608"),
            Some("dotnet build is green"),
            Some("dotnet build"),
            None,
            5,
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
        // The recall query is derived from description + tags by default.
        assert_eq!(
            v["recall"]["query"].as_str().unwrap(),
            "Fix the failing build ci"
        );
        assert!(v["next"].is_object(), "the next cursor is printed");
    }

    #[test]
    fn missing_acceptance_warns_but_still_founds_the_task() {
        let conn = db::open_in_memory_for_test();
        let v = begin_value(
            &conn,
            &cfg(),
            "Add a config flag",
            &[],
            &[],
            Some("proj"),
            None,
            None,
            None,
            None,
            None,
            None,
            5,
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
    fn explicit_query_overrides_the_derived_one() {
        let conn = db::open_in_memory_for_test();
        let v = begin_value(
            &conn,
            &cfg(),
            "Some long verbose task description",
            &["tagx".to_string()],
            &[],
            Some("proj"),
            None,
            None,
            None,
            None,
            None,
            Some("windows path gate"),
            5,
        )
        .expect("begin succeeds");
        assert_eq!(v["recall"]["query"].as_str().unwrap(), "windows path gate");
    }

    #[test]
    fn begin_exposes_the_folded_recall_duration_for_nested_telemetry() {
        // `begin` folds a `recall` inside it, so that recall never passes
        // through the MCP `call_tool` telemetry choke point. To let each caller
        // emit a separate, nested telemetry event for the folded lookup,
        // `begin_value` must surface how long the internal recall took.
        let conn = db::open_in_memory_for_test();
        let v = begin_value(
            &conn,
            &cfg(),
            "Fix the failing build",
            &["ci".to_string()],
            &[],
            Some("proj"),
            None,
            None,
            None,
            Some("dotnet build"),
            None,
            None,
            5,
        )
        .expect("begin succeeds");

        assert!(
            v["recall"]["duration_ms"].is_u64(),
            "the folded recall reports its own duration for nested telemetry, got {}",
            v["recall"]
        );
    }
}
