use std::collections::HashSet;

use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use super::next_actions::next_actions;
use crate::commands::shared::{
    cited_memories, item_snippet, memory_handle, project_head, project_path,
};
use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Task;
use crate::infrastructure::project::{detect_current_project, resolve_file_link};

const RECENT_NOTES: usize = 5;
const FILE_MEMORIES: usize = 8;
const RESULT_CHARS: usize = 200;

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

/// The pending task an agent most plausibly left off on: a started one first,
/// then the one with the latest activity.
fn latest_task(conn: &Connection, cfg: &Config) -> Result<Option<Task>> {
    let (project, _) = detect_current_project(conn, cfg)?;
    let tasks = db::list_tasks(conn, Some(&project))?;
    let last = db::last_event_times(conn).unwrap_or_default();
    Ok(tasks.into_iter().max_by_key(|t| {
        (
            t.is_active(),
            last.get(&t.uuid)
                .copied()
                .unwrap_or(t.modified)
                .max(t.modified),
        )
    }))
}

/// Everything an agent needs to pick a task back up after a context reset, in
/// one compact payload: the brief, progress with recorded results, what it was
/// doing, recent notes, links, the memories it cited or that are anchored to
/// its files, and the next tool calls. With no `id` it resumes the current
/// project's most recently active pending task.
pub fn resume_value(conn: &Connection, cfg: &Config, id: Option<&str>) -> Result<Value> {
    let task = match id {
        Some(id) => db::resolve_task(conn, id)?,
        None => match latest_task(conn, cfg)? {
            Some(t) => t,
            None => {
                return Ok(json!({
                    "task": null,
                    "message": "no pending task in this project",
                    "next_actions": [{
                        "tool": "begin",
                        "args": { "description": "<what you are about to do>" },
                        "why": "start tracking the work before you begin",
                    }],
                }));
            }
        },
    };

    let guide = db::get_guide_fields(conn, &task.uuid)?;
    let steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;
    let done_steps: Vec<Value> = steps
        .iter()
        .enumerate()
        .filter(|(_, s)| s.done)
        .map(|(i, s)| {
            json!({
                "n": i + 1,
                "text": s.text,
                "result": s.result.as_deref().map(|r| clip(r, RESULT_CHARS)),
            })
        })
        .collect();
    let open_steps: Vec<Value> = steps
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.done)
        .map(|(i, s)| json!({ "n": i + 1, "text": s.text, "intent": s.intent }))
        .collect();
    let criteria: Vec<Value> = acceptance
        .iter()
        .enumerate()
        .map(|(i, a)| json!({ "n": i + 1, "text": a.text, "done": a.done, "verify": a.verify_cmd }))
        .collect();

    let head = project_head(conn, &task.project);
    let validated = json!({
        "commit": guide.validated_commit,
        "at": guide.validated_at,
        "at_head": guide.validated_commit.is_some() && guide.validated_commit == head,
    });

    let doing = db::latest_doing(conn, &task.uuid)?
        .map(|d| json!({ "text": d.text, "at": d.at.to_rfc3339() }));
    let annotations = db::get_annotations(conn, &task.uuid)?;
    let notes: Vec<Value> = annotations
        .iter()
        .rev()
        .take(RECENT_NOTES)
        .map(|a| json!({ "id": a.id, "kind": a.kind, "text": a.text, "at": a.entry.to_rfc3339() }))
        .collect();
    let links: Vec<Value> = db::get_links(conn, &task.uuid)?
        .into_iter()
        .map(|l| json!({ "url": l.url, "label": l.label }))
        .collect();

    let cited = cited_memories(conn, &task.uuid);
    let mut seen: HashSet<uuid::Uuid> = cited.iter().map(|m| m.uuid).collect();
    let cited: Vec<Value> = cited
        .iter()
        .map(|m| json!({ "label": memory_handle(m), "preview": item_snippet(m, 120) }))
        .collect();

    let root = project_path(conn, &task.project);
    let mut file_memories = Vec::new();
    'files: for file in db::get_task_files(conn, &task.uuid)? {
        let resolved = match &root {
            Some(r) => resolve_file_link(&file, Some(r.as_path()), r),
            None => file.clone(),
        };
        let prefix = resolved.ends_with('/');
        for m in db::find_items_by_file(conn, &resolved, prefix)? {
            if m.kind != "memory" || !seen.insert(m.uuid) {
                continue;
            }
            file_memories.push(json!({
                "label": memory_handle(&m),
                "file": file,
                "preview": item_snippet(&m, 120),
            }));
            if file_memories.len() >= FILE_MEMORIES {
                break 'files;
            }
        }
    }

    Ok(json!({
        "task": {
            "uuid": task.uuid.to_string(),
            "id": task.id,
            "description": task.description,
            "project": task.project,
            "status": task.status.to_string(),
            "priority": task.priority.as_ref().map(|p| format!("{p:?}")),
            "tags": task.tags,
        },
        "assignment": guide.assignment,
        "rationale": guide.rationale,
        "progress": format!("{}/{} steps, {}/{} acceptance", done_steps.len(), steps.len(), acceptance.iter().filter(|a| a.done).count(), acceptance.len()),
        "done_steps": done_steps,
        "open_steps": open_steps,
        "acceptance": criteria,
        "validated": validated,
        "doing": doing,
        "recent_notes": notes,
        "links": links,
        "cited_memories": cited,
        "file_memories": file_memories,
        "next_actions": next_actions(conn, &task)?,
    }))
}
