use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::git;
use crate::infrastructure::model::format_duration;

pub fn start(conn: &Connection, cfg: &Config, id_or_uuid: &str) -> Result<()> {
    let mut task = db::resolve_task(conn, id_or_uuid)?;

    if task.is_active() {
        println!(
            "Task {} is already active (running for {}).",
            task.id.unwrap_or(0),
            format_duration(task.total_time_spent())
        );
        return Ok(());
    }

    task.started_at = Some(Utc::now());
    task.modified = Utc::now();
    db::update_task(conn, &task)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;

    println!(
        "Started task {}: {}",
        task.id.unwrap_or(0),
        task.description
    );
    Ok(())
}

pub fn stop(conn: &Connection, cfg: &Config, id_or_uuid: &str) -> Result<()> {
    let mut task = db::resolve_task(conn, id_or_uuid)?;

    let Some(started) = task.started_at else {
        println!("Task {} is not active.", task.id.unwrap_or(0));
        return Ok(());
    };

    let session = (Utc::now() - started).num_seconds().max(0);
    task.time_spent += session;
    task.started_at = None;
    task.modified = Utc::now();
    db::update_task(conn, &task)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;

    println!(
        "Stopped task {} (this session: {}, total: {})",
        task.id.unwrap_or(0),
        format_duration(session),
        format_duration(task.time_spent)
    );

    // If this task has a tied branch, snapshot its changed files.
    if let Some(branch_rec) = db::get_task_branch(conn, &task.uuid) {
        let project_path = db::get_project(conn, &task.project)
            .ok()
            .flatten()
            .and_then(|p| p.path);

        if let Some(path) = project_path {
            let repo = std::path::Path::new(&path);
            match git::changed_files(repo, &branch_rec.branch) {
                Ok((base, files)) => {
                    let n = files.len();
                    let _ = db::log_branch_changes(conn, &task.uuid, &base, &files);
                    println!(
                        "Logged {} changed file{} on branch '{}'.",
                        n,
                        if n == 1 { "" } else { "s" },
                        branch_rec.branch
                    );
                }
                Err(e) => {
                    eprintln!("Warning: could not snapshot branch changes: {e:#}");
                }
            }
        }
    }

    Ok(())
}
