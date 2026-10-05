use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use super::support::print_checklist;
use crate::commands::shared::print_json;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

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

    print_checklist("Steps (tick with `step done <id> N`):", &steps);
    print_checklist(
        "Acceptance criteria (tick with `step done <id> N --kind acceptance`):",
        &acceptance,
    );
    Ok(())
}
