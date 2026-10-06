use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::commands::shared::{item_snippet, memory_handle, print_json};
use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::db::MemoryUseKind;
use crate::infrastructure::model::Task;

const NEXT_MEMORY_LIMIT: usize = 3;

fn relevant_memories(conn: &Connection, task: &Task) -> Vec<(String, String)> {
    db::find_similar_strong_memories(conn, &task.description, &task.tags)
        .unwrap_or_default()
        .into_iter()
        .take(NEXT_MEMORY_LIMIT)
        .map(|item| {
            let _ = db::record_memory_use(conn, &item.uuid, &task.uuid, MemoryUseKind::Surfaced);
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
