use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::db;
use crate::portable::{
    AnnotationDto, BUNDLE_FORMAT, BUNDLE_VERSION, Bundle, ChecklistDto, FileDto, LinkDto,
    TaskEnvelope,
};

/// Export a task (and its full dependency closure) to a portable copy-paste blob.
///
/// The root is resolved by display id or uuid prefix; its dependency closure
/// (the task plus every transitive blocker) is serialized together so the bundle
/// is self-contained and dependency edges survive the trip. When `output` is set
/// the blob is written there; otherwise it is printed to stdout.
pub fn run(conn: &Connection, id: &str, output: Option<&Path>) -> Result<()> {
    let root = db::resolve_task(conn, id)?;

    // The closure includes the root itself (depth 0) plus all transitive blockers.
    let closure = db::dependency_closure(conn, &root.uuid)?;

    let mut tasks = Vec::with_capacity(closure.len());
    for uuid in &closure {
        let task = db::get_task_by_uuid_prefix(conn, &uuid.to_string())?
            .with_context(|| format!("task {uuid} vanished during export"))?;

        let annotations = db::get_annotations(conn, &task.uuid)?
            .into_iter()
            .map(|a| AnnotationDto {
                text: a.text,
                kind: a.kind,
                author: a.author,
                target_kind: a.target_kind,
                target_id: a.target_id,
            })
            .collect();

        let checklist = db::get_checklist(conn, &task.uuid)?
            .into_iter()
            .map(|c| ChecklistDto {
                text: c.text,
                done: c.done,
                kind: c.kind,
                source: c.source,
                intent: c.intent,
                verify_cmd: c.verify_cmd,
                result: c.result,
                done_commit: c.done_commit,
            })
            .collect();

        let links = db::get_links(conn, &task.uuid)?
            .into_iter()
            .map(|l| LinkDto {
                url: l.url,
                label: l.label,
            })
            .collect();

        let files = db::get_task_files_sourced(conn, &task.uuid)?
            .into_iter()
            .map(|(path, source)| FileDto { path, source })
            .collect();

        // Only keep dependency edges whose target is also in the closure (it
        // always is, by construction) so the bundle is internally consistent.
        let blocked_by = db::get_dependency_uuids(conn, &task.uuid)?
            .into_iter()
            .filter(|d| closure.contains(d))
            .collect();

        tasks.push(TaskEnvelope {
            uuid: task.uuid,
            description: task.description,
            project: task.project,
            status: task.status,
            priority: task.priority,
            due: task.due,
            entry: task.entry,
            tags: task.tags,
            estimate_mins: task.estimate_mins,
            recur: task.recur,
            blocked_by,
            annotations,
            checklist,
            links,
            files,
        });
    }

    let bundle = Bundle {
        format: BUNDLE_FORMAT.into(),
        version: BUNDLE_VERSION,
        exported_at: chrono::Utc::now(),
        root: root.uuid,
        tasks,
    };
    let blob = bundle.encode()?;
    let extra = bundle.tasks.len().saturating_sub(1);

    match output {
        Some(path) => {
            std::fs::write(path, format!("{blob}\n"))
                .with_context(|| format!("writing blob to {}", path.display()))?;
            let dep_note = if extra > 0 {
                format!(
                    " (+{extra} dependency task{})",
                    if extra == 1 { "" } else { "s" }
                )
            } else {
                String::new()
            };
            eprintln!(
                "Exported task {}{dep_note} to {}",
                root.id.unwrap_or(0),
                path.display()
            );
        }
        None => {
            // The blob alone goes to stdout so it can be piped/redirected cleanly;
            // the human-readable hint goes to stderr.
            println!("{blob}");
            if extra > 0 {
                eprintln!(
                    "Exported task {} with {extra} dependency task{}. Import with `sara import`.",
                    root.id.unwrap_or(0),
                    if extra == 1 { "" } else { "s" }
                );
            } else {
                eprintln!(
                    "Exported task {}. Import with `sara import`.",
                    root.id.unwrap_or(0)
                );
            }
        }
    }
    Ok(())
}
