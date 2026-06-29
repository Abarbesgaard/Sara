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

/// One row in the project browser: a project plus the metadata shown for it.
pub(super) struct ProjectRow {
    pub(super) name: String,
    pub(super) goal: Option<String>,
    pub(super) stack: Option<String>,
    pub(super) pending: u32,
    pub(super) done: u32,
    pub(super) last_activity: Option<DateTime<Utc>>,
}

pub(super) struct ProjectListState {
    pub(super) rows: Vec<ProjectRow>,
    pub(super) selected: usize,
    pub(super) scroll: u16,
}

/// Browse all projects in a scrollable TUI; pressing Enter drills into the
/// selected project's board, and quitting the board returns to this list.
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

        let mut terminal = crate::infrastructure::tui::init_terminal()?;
        let action = render::list_loop(&mut terminal, &mut st)?;
        crate::infrastructure::tui::restore_terminal()?;

        selected = st.selected;
        scroll = st.scroll;

        match action {
            ProjectAction::Quit => break,
            ProjectAction::Open(name) => {
                // Reuse the existing per-project board as the drill-in target,
                // then loop back to a freshly rebuilt project list.
                crate::commands::board::run(conn, cfg, Some(&name))?;
            }
        }
    }
    Ok(())
}

/// Collect every known project with its metadata and task counts, ordered by
/// most-recent activity (projects with no tasks sort last), then by name.
fn build_rows(conn: &Connection) -> Result<Vec<ProjectRow>> {
    let names = db::project_names(conn)?;
    let mut rows = Vec::with_capacity(names.len());
    for name in names {
        let profile = db::get_project(conn, &name)?;
        let stats = db::project_stats(conn, &name)?;
        let last_activity = db::project_last_activity(conn, &name)?;
        rows.push(ProjectRow {
            goal: profile.as_ref().and_then(|p| p.goal.clone()),
            stack: profile.as_ref().and_then(|p| p.stack.clone()),
            pending: stats.pending,
            done: stats.completed_total,
            last_activity,
            name,
        });
    }
    sort_rows(&mut rows);
    Ok(rows)
}

/// Most-recently-active project first; projects with no activity (`None`) sort
/// last; ties broken by name for stable output.
fn sort_rows(rows: &mut [ProjectRow]) {
    rows.sort_by(|a, b| {
        b.last_activity
            .cmp(&a.last_activity)
            .then_with(|| a.name.cmp(&b.name))
    });
}

pub(super) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Compact relative age, e.g. "just now", "5m ago", "3h ago", "2d ago".
pub(super) fn rel_time(dt: DateTime<Utc>) -> String {
    let secs = (Utc::now() - dt).num_seconds().max(0);
    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    match secs {
        s if s < MIN => "just now".to_string(),
        s if s < HOUR => format!("{}m ago", s / MIN),
        s if s < DAY => format!("{}h ago", s / HOUR),
        s if s < 30 * DAY => format!("{}d ago", s / DAY),
        s if s < 365 * DAY => format!("{}mo ago", s / (30 * DAY)),
        s => format!("{}y ago", s / (365 * DAY)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, last: Option<DateTime<Utc>>) -> ProjectRow {
        ProjectRow {
            name: name.to_string(),
            goal: None,
            stack: None,
            pending: 0,
            done: 0,
            last_activity: last,
        }
    }

    #[test]
    fn sort_rows_orders_by_recent_activity_then_name() {
        let now = Utc::now();
        let mut rows = vec![
            row("zeta", Some(now - chrono::Duration::days(5))),
            row("alpha", None),
            row("beta", Some(now)),
            row("gamma", None),
        ];
        sort_rows(&mut rows);
        let order: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        // beta (newest) first, then zeta (older), then None-activity by name.
        assert_eq!(order, ["beta", "zeta", "alpha", "gamma"]);
    }

    #[test]
    fn rel_time_buckets() {
        let now = Utc::now();
        assert_eq!(rel_time(now), "just now");
        assert_eq!(rel_time(now - chrono::Duration::minutes(5)), "5m ago");
        assert_eq!(rel_time(now - chrono::Duration::hours(3)), "3h ago");
        assert_eq!(rel_time(now - chrono::Duration::days(2)), "2d ago");
    }

    #[test]
    fn truncate_adds_ellipsis_only_when_needed() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefgh", 4), "abc…");
    }
}
