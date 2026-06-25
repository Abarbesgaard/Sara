use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;
use uuid::Uuid;

use crate::config::Config;
use crate::db;
use crate::model::Task;
use crate::portable::Bundle;

/// Import a task bundle from a portable copy-paste blob.
///
/// `source` may be a path to a file containing the blob, the blob string itself,
/// or `None` to read from stdin. Every task gets a fresh uuid and display id;
/// dependency edges are remapped within the bundle; the timer is reset and
/// urgency recomputed. `project_override` reassigns every imported task.
pub fn run(
    conn: &mut Connection,
    cfg: &Config,
    source: Option<&str>,
    project_override: Option<&str>,
) -> Result<()> {
    let raw = read_source(source)?;
    let bundle = Bundle::decode(&raw)?;

    let tx = conn.transaction()?;
    let mut id_map: HashMap<Uuid, Uuid> = HashMap::with_capacity(bundle.tasks.len());

    // Pass 1 — insert every task (fresh uuid + display id) and its child rows.
    for env in &bundle.tasks {
        let project = project_override.unwrap_or(&env.project).to_string();
        let mut task = Task::new(env.description.clone(), project);
        task.status = env.status.clone();
        task.priority = env.priority.clone();
        task.due = env.due;
        task.entry = env.entry;
        task.modified = chrono::Utc::now();
        task.tags = env.tags.clone();
        task.estimate_mins = env.estimate_mins;
        task.recur = env.recur.clone();
        // started_at / time_spent stay at their Task::new defaults (timer reset).

        db::insert_task(&tx, &mut task)?;
        id_map.insert(env.uuid, task.uuid);

        for a in &env.annotations {
            db::add_annotation_full(
                &tx,
                &task.uuid,
                &a.text,
                &a.kind,
                &a.author,
                a.target_kind.as_deref(),
                a.target_id.as_deref(),
                false,
            )?;
        }
        for c in &env.checklist {
            let step_id = db::add_step(
                &tx,
                &task.uuid,
                &c.text,
                c.intent.as_deref(),
                &c.kind,
                &c.source,
                c.verify_cmd.as_deref(),
            )?;
            if c.done {
                db::set_step_done(
                    &tx,
                    step_id,
                    true,
                    c.result.as_deref(),
                    c.done_commit.as_deref(),
                )?;
            }
        }
        for l in &env.links {
            db::add_link(&tx, &task.uuid, &l.url, l.label.as_deref())?;
        }
        if !env.files.is_empty() {
            let files: Vec<(String, String)> = env
                .files
                .iter()
                .map(|f| (f.path.clone(), f.source.clone()))
                .collect();
            db::set_task_files_sourced(&tx, &task.uuid, &files)?;
        }
    }

    // Pass 2 — remap dependency edges now that every task exists.
    for env in &bundle.tasks {
        let new_task = id_map[&env.uuid];
        for dep in &env.blocked_by {
            if let Some(new_dep) = id_map.get(dep) {
                db::add_dependency(&tx, &new_task, new_dep)?;
            }
        }
    }

    // Pass 3 — recompute urgency (depends on the freshly created edges).
    for new_uuid in id_map.values() {
        db::refresh_urgency(&tx, &cfg.urgency, new_uuid)?;
    }

    tx.commit()?;

    report(conn, &bundle, &id_map, project_override)?;
    Ok(())
}

/// Print a short summary of what was imported.
fn report(
    conn: &Connection,
    bundle: &Bundle,
    id_map: &HashMap<Uuid, Uuid>,
    project_override: Option<&str>,
) -> Result<()> {
    let total = bundle.tasks.len();
    let extra = total.saturating_sub(1);

    let root_line = id_map
        .get(&bundle.root)
        .and_then(|u| {
            db::get_task_by_uuid_prefix(conn, &u.to_string())
                .ok()
                .flatten()
        })
        .map(|t| {
            format!(
                "Imported task {} \"{}\" into project '{}'",
                t.id.unwrap_or(0),
                truncate(&t.description, 60),
                t.project
            )
        })
        .unwrap_or_else(|| "Imported task".to_string());

    println!("{root_line}");
    if extra > 0 {
        println!(
            "  + {extra} dependency task{} (edges remapped)",
            if extra == 1 { "" } else { "s" }
        );
    }
    if let Some(p) = project_override {
        println!("  reassigned all imported tasks to project '{p}'");
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// Resolve the raw blob text from a file path, a literal argument, or stdin.
fn read_source(source: Option<&str>) -> Result<String> {
    match source {
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("reading task blob from stdin")?;
            Ok(buf)
        }
        Some(s) => {
            let path = Path::new(s);
            if path.is_file() {
                std::fs::read_to_string(path)
                    .with_context(|| format!("reading task blob from {}", path.display()))
            } else {
                Ok(s.to_string())
            }
        }
    }
}
