use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use super::commands::{all_commands, nth_step};
use crate::infrastructure::db;

pub fn verify_value(conn: &Connection, id: &str, step: Option<usize>) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;

    let cmds: Vec<String> = match step {
        Some(n) => nth_step(&steps, n)?.verify_cmd.iter().cloned().collect(),
        None => all_commands(conn, &task, &steps, &acceptance)?,
    };

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
