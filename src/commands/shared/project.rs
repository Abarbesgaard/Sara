use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::domain::Task;
use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::git;

pub fn project_path(conn: &Connection, project: &str) -> Option<PathBuf> {
    db::get_project(conn, project)
        .ok()
        .flatten()
        .and_then(|p| p.path)
        .map(PathBuf::from)
}

pub fn project_head(conn: &Connection, project: &str) -> Option<String> {
    git::head_commit(&project_path(conn, project)?)
}

pub fn require_project_head(conn: &Connection, project: &str) -> Result<String> {
    let path = project_path(conn, project);
    if let Some(head) = path.as_deref().and_then(git::head_commit) {
        return Ok(head);
    }
    match path {
        Some(p) if !p.exists() => anyhow::bail!(
            "project path {} no longer exists — re-run `sara init` in the new location",
            p.display()
        ),
        Some(p) if git::is_repo(&p) => anyhow::bail!(
            "project has no commits yet — validation stamps a commit; \
             make the first commit, then re-run validate"
        ),
        _ => anyhow::bail!("task's project is not in a git repo"),
    }
}

pub fn parse_due(s: &str, cfg: &Config) -> Option<DateTime<Utc>> {
    crate::infrastructure::util::dates::parse_due(s, &cfg.date_dialect)
}

pub fn guard_branch_mutation(
    conn: &Connection,
    matched: db::IdMatch,
    task: &Task,
    force: bool,
) -> Result<()> {
    if force || matched == db::IdMatch::UuidPrefix {
        return Ok(());
    }
    let Some(rec) = db::get_task_branch(conn, &task.uuid) else {
        return Ok(());
    };
    let Some(path) = project_path(conn, &task.project) else {
        return Ok(());
    };
    let Some(current) = git::current_branch(&path) else {
        return Ok(());
    };
    if current != rec.branch {
        anyhow::bail!(
            "refusing to act on task {} by display id — it is tied to branch '{}' but the \
             project is currently on '{}'. Recycled display ids can point at a different task \
             after recompaction. Re-run with the stable uuid `{}` (or pass --force).",
            task.id.unwrap_or(0),
            rec.branch,
            current,
            stable_id(task)
        );
    }
    Ok(())
}

/// A uuid prefix that can never be read as a display id.
fn stable_id(task: &Task) -> String {
    let uuid = task.uuid.to_string();
    let short = &uuid[..8];
    if short.parse::<i64>().is_ok() {
        uuid[..13].to_string()
    } else {
        short.to_string()
    }
}

pub fn resolve_files(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .map(|p| crate::infrastructure::project::resolve_file_link_here(p))
        .collect()
}
