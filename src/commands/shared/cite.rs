use anyhow::Result;
use rusqlite::Connection;
use uuid::Uuid;

use crate::commands::shared::memory_handle;
use crate::infrastructure::db::{self, MemoryUseKind};
use crate::infrastructure::model::Item;

/// Memories an agent says it used on a task, resolved but not yet recorded.
struct Citation {
    task: Uuid,
    memories: Vec<Item>,
}

/// Resolves the task and every label up front, so one bad label fails the
/// whole call before a step is ticked, a task is closed or a row is written.
fn prepare_citation(
    conn: &Connection,
    task_id: &str,
    labels: &[String],
) -> Result<Option<Citation>> {
    let labels: Vec<&str> = labels
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if labels.is_empty() {
        return Ok(None);
    }
    let task = db::resolve_task(conn, task_id)?.uuid;
    let mut memories = Vec::new();
    let mut unknown = Vec::new();
    for label in labels {
        match db::get_item_by_handle(conn, label) {
            Ok(item) if item.kind == "memory" => {
                if !memories.iter().any(|m: &Item| m.uuid == item.uuid) {
                    memories.push(item);
                }
            }
            _ => unknown.push(label),
        }
    }
    if !unknown.is_empty() {
        anyhow::bail!(
            "Unknown memory label(s) in `used`: {}. Nothing was recorded; cite active memories such as m12.",
            unknown.join(", ")
        );
    }
    Ok(Some(Citation { task, memories }))
}

/// Records the citation as `cited` memory uses and returns the memory handles.
fn record_citation(conn: &Connection, citation: &Citation) -> Result<Vec<String>> {
    let tx = conn.unchecked_transaction()?;
    for item in &citation.memories {
        db::record_memory_use(&tx, &item.uuid, &citation.task, MemoryUseKind::Cited)?;
    }
    tx.commit()?;
    Ok(citation.memories.iter().map(memory_handle).collect())
}

/// Adds `cited` to a command's JSON result when memories were cited.
fn attach_cited(value: &mut serde_json::Value, cited: Option<Vec<String>>) {
    if let (Some(cited), Some(obj)) = (cited, value.as_object_mut()) {
        obj.insert("cited".to_string(), serde_json::json!(cited));
    }
}

/// Runs `f` between preparing and recording a citation, so a failing command
/// records nothing and a bad label stops the command before it runs.
pub fn with_citation(
    conn: &Connection,
    task_id: &str,
    labels: &[String],
    f: impl FnOnce() -> Result<serde_json::Value>,
) -> Result<serde_json::Value> {
    let citation = prepare_citation(conn, task_id, labels)?;
    let mut value = f()?;
    let cited = citation
        .as_ref()
        .map(|c| record_citation(conn, c))
        .transpose()?;
    attach_cited(&mut value, cited);
    Ok(value)
}

/// Prints the `cited` handles of a command result, if any.
pub fn print_cited(value: &serde_json::Value) {
    let cited = crate::commands::shared::json_strs(&value["cited"]);
    if !cited.is_empty() {
        println!("📎 Cited: {}", cited.join(", "));
    }
}

/// The memories a task cited, oldest citation first.
pub fn cited_memories(conn: &Connection, task: &Uuid) -> Vec<Item> {
    db::memory_uses_for_task(conn, task)
        .unwrap_or_default()
        .into_iter()
        .filter(|u| u.kind == MemoryUseKind::Cited)
        .filter_map(|u| db::get_item_by_uuid(conn, &u.item_uuid.to_string()).ok())
        .collect()
}

/// `m12 <snippet>` lines for showing a task's citations.
pub fn cited_labels(conn: &Connection, task: &Uuid) -> Vec<String> {
    cited_memories(conn, task)
        .iter()
        .map(|m| {
            format!(
                "{} {}",
                memory_handle(m),
                crate::commands::shared::item_snippet(m, 80)
            )
        })
        .collect()
}
