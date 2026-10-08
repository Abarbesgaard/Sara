use anyhow::Result;
use rusqlite::Connection;

use chrono::{DateTime, Utc};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;

mod render;

pub(super) enum ProjectAction {
    Quit,
    Open(String),
}

pub(super) struct ProjectRow {
    pub(super) name: String,
    pub(super) goal: Option<String>,
    pub(super) stack: Option<String>,
    pub(super) pending: u32,
    pub(super) active: u32,
    pub(super) done: u32,
    pub(super) stale: u32,
    pub(super) feedback: u32,
    pub(super) last_activity: Option<DateTime<Utc>>,
}

pub(super) struct ProjectListState {
    pub(super) rows: Vec<ProjectRow>,
    pub(super) selected: usize,
    pub(super) scroll: u16,
}

pub fn run(conn: &Connection, cfg: &Config) -> Result<()> {
    let mut selected = 0usize;
    let mut scroll = 0u16;

    loop {
        let rows = build_rows(conn)?;
        if rows.is_empty() {
            println!("No projects yet. Run `sara init` in a repository to register one.");
            return Ok(());
        }

        let mut st = ProjectListState {
            selected: selected.min(rows.len() - 1),
            scroll,
            rows,
        };

        let action = crate::infrastructure::tui::with_terminal(|t| render::list_loop(t, &mut st))?;

        selected = st.selected;
        scroll = st.scroll;

        match action {
            ProjectAction::Quit => break,
            ProjectAction::Open(name) => {
                crate::commands::board::run(conn, cfg, Some(&name), false)?;
            }
        }
    }
    Ok(())
}

fn build_rows(conn: &Connection) -> Result<Vec<ProjectRow>> {
    let names = db::project_names(conn)?;
    let mut rows = Vec::with_capacity(names.len());
    for name in names {
        let profile = db::get_project(conn, &name)?;
        let stats = db::project_stats(conn, &name)?;
        let last_activity = db::project_last_activity(conn, &name)?;
        let head = profile
            .as_ref()
            .and_then(|p| p.path.as_deref())
            .and_then(|p| crate::infrastructure::git::head_commit(std::path::Path::new(p)));
        let badges = db::project_badges(conn, &name, head.as_deref())?;
        rows.push(ProjectRow {
            goal: profile.as_ref().and_then(|p| p.goal.clone()),
            stack: profile.as_ref().and_then(|p| p.stack.clone()),
            pending: stats.pending,
            active: stats.active,
            done: stats.completed_total,
            stale: badges.stale,
            feedback: badges.feedback,
            last_activity,
            name,
        });
    }
    sort_rows(&mut rows);
    Ok(rows)
}

fn sort_rows(rows: &mut [ProjectRow]) {
    rows.sort_by(|a, b| {
        b.last_activity
            .cmp(&a.last_activity)
            .then_with(|| a.name.cmp(&b.name))
    });
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/projects/mod.rs"]
mod tests;
