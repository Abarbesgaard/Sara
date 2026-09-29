use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::commands;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

mod render;
mod types;

use types::{RECALL_STEP_INTENT, RECALL_STEP_TEXT};

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

    let assignment_text = assignment
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| description.trim());
    commands::guide::assignment_value(conn, &id, assignment_text)?;

    let rationale_text = rationale.map(str::trim).filter(|s| !s.is_empty());
    if let Some(why) = rationale_text {
        commands::guide::rationale_value(conn, &id, why)?;
    }

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
