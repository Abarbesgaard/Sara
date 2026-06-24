use anyhow::Result;
use rusqlite::Connection;
use std::io::{self, Write};

use crate::config::Config;
use crate::project::project_identity_for_dir;

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
    let task_count = crate::db::count_project_tasks(conn, &name)?;
    let profile = crate::db::get_project(conn, &name)?;

    if task_count == 0 && profile.is_none() {
        println!("Nothing to reset: project '{name}' has no tasks or profile.");
        return Ok(());
    }

    if !yes {
        println!(
            "This will permanently delete project '{name}':\n  \
             • {task_count} task(s) and all their files, links, comments and history\n  \
             • the project profile (you'll need to run `sara init` again)"
        );
        print!("Type the project name to confirm: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if input.trim() != name {
            println!("Aborted — name did not match.");
            return Ok(());
        }
    }

    let deleted = crate::db::reset_project(conn, &name)?;
    println!("✔ Reset project '{name}': removed {deleted} task(s) and its profile.");
    println!("Run `sara init` to set it up again.");
    Ok(())
}
