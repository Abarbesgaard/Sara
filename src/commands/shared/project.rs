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
    id_input: &str,
    task: &Task,
    force: bool,
) -> Result<()> {
    if force || id_input.parse::<i64>().is_err() {
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
            &task.uuid.to_string()[..8]
        );
    }
    Ok(())
}

pub fn resolve_files(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .map(|p| crate::infrastructure::project::resolve_file_link_here(p))
        .collect()
}
