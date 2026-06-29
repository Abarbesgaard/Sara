use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::infrastructure::db;
use crate::infrastructure::git;

pub fn run(conn: &Connection, id_or_uuid: &str, clear: bool) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;

    if clear {
        db::clear_task_branch(conn, &task.uuid)?;
        println!("Removed branch tie from task {}.", task.id.unwrap_or(0));
        return Ok(());
    }

    // Resolve the project's git root.
    let project_path = db::get_project(conn, &task.project)?
        .and_then(|p| p.path)
        .with_context(|| {
            format!(
                "Project '{}' has no recorded path. Make sure you've run `sara project init` inside a git repo.",
                task.project
            )
        })?;

    let repo = std::path::Path::new(&project_path);

    let branch = git::current_branch(repo).with_context(|| {
        "Could not determine current branch. Make sure you're inside a git repo and not in a detached HEAD state."
    })?;

    db::set_task_branch(conn, &task.uuid, &branch)?;
    println!("Tied task {} to branch '{}'.", task.id.unwrap_or(0), branch);
    println!(
        "Run `sara stop {}` to snapshot changed files.",
        task.id.unwrap_or(0)
    );
    Ok(())
}
