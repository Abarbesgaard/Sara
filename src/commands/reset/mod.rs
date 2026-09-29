use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::project::project_identity_for_dir;

mod render;

/// Resolve the project name for the current directory *without* registering it
/// (unlike `detect_current_project`, which upserts a `last_seen` row).
fn resolve_name(cfg: &Config, override_name: Option<&str>) -> Result<String> {
    if let Some(name) = override_name {
        return Ok(name.to_string());
    }
    let cwd = std::env::current_dir()?;
    let (name, _path) = project_identity_for_dir(&cwd, cfg);
    Ok(name)
}

pub fn run(
    conn: &mut Connection,
    cfg: &Config,
    project_override: Option<&str>,
    yes: bool,
) -> Result<()> {
    let name = resolve_name(cfg, project_override)?;
    let task_count = crate::infrastructure::db::count_project_tasks(conn, &name)?;
    let profile = crate::infrastructure::db::get_project(conn, &name)?;

    if task_count == 0 && profile.is_none() {
        render::print_nothing_to_reset(&name);
        return Ok(());
    }

    if !yes && !render::confirm(&name, task_count)? {
        render::print_aborted();
        return Ok(());
    }

    let deleted = crate::infrastructure::db::reset_project(conn, &name)?;
    render::print_reset(&name, deleted);
    Ok(())
}
