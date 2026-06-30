use anyhow::Result;
use rusqlite::Connection;
use std::io::{self, Write};
use tui_textarea::TextArea;

use chrono::{DateTime, Utc};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Project;

mod render;

pub(super) enum ProjectAction {
    Quit,
    Open(String),
    Edit(String),
    Delete(String),
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
/// selected project's board, `e` edits its profile (incl. rename), `d` deletes
/// it, and quitting a sub-screen returns to this list.
pub fn run(conn: &mut Connection, cfg: &Config) -> Result<()> {
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
            ProjectAction::Edit(name) => {
                edit_project(conn, &name)?;
            }
            ProjectAction::Delete(name) => {
                confirm_and_delete(conn, &name)?;
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

// ── project editor ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ProjField {
    Name,
    Goal,
    Stack,
    Conventions,
    Notes,
}

pub(super) const PROJ_FIELDS: [ProjField; 5] = [
    ProjField::Name,
    ProjField::Goal,
    ProjField::Stack,
    ProjField::Conventions,
    ProjField::Notes,
];

impl ProjField {
    pub(super) fn label(self) -> &'static str {
        match self {
            ProjField::Name => "Name",
            ProjField::Goal => "Goal",
            ProjField::Stack => "Stack",
            ProjField::Conventions => "Conventions",
            ProjField::Notes => "Notes",
        }
    }
}

pub(super) struct ProjectEditState {
    /// Name the project currently lives under (updated after a successful rename).
    pub(super) name: String,
    pub(super) goal: Option<String>,
    pub(super) stack: Option<String>,
    pub(super) conventions: Option<String>,
    pub(super) notes: Option<String>,
    /// Profile path, preserved across saves (never edited here).
    pub(super) path: Option<String>,
    pub(super) task_count: usize,
    pub(super) selected: usize,
    pub(super) editing: bool,
    pub(super) editor: TextArea<'static>,
    /// Transient status / error line shown under the fields.
    pub(super) status: Option<String>,
}

impl ProjectEditState {
    pub(super) fn field_value(&self, f: ProjField) -> &str {
        match f {
            ProjField::Name => &self.name,
            ProjField::Goal => self.goal.as_deref().unwrap_or(""),
            ProjField::Stack => self.stack.as_deref().unwrap_or(""),
            ProjField::Conventions => self.conventions.as_deref().unwrap_or(""),
            ProjField::Notes => self.notes.as_deref().unwrap_or(""),
        }
    }

    /// Build a profile for persistence. Content fields carry their current
    /// (possibly edited) values; github/path fields are passed through so
    /// `save_project_profile`'s COALESCE keeps whatever is already stored.
    pub(super) fn to_profile(&self) -> Project {
        Project {
            name: self.name.clone(),
            path: self.path.clone(),
            goal: self.goal.clone(),
            stack: self.stack.clone(),
            conventions: self.conventions.clone(),
            notes: self.notes.clone(),
            initialized_at: None,
            last_seen: None,
            github_repo: None,
            github_login: None,
            github_sync_scope: None,
        }
    }
}

/// Open the per-project editor for `name`: edit goal/stack/conventions/notes in
/// place, or rename the project (which cascades to every task). Reuses the
/// init/restore-per-screen pattern so it nests cleanly under the browser.
fn edit_project(conn: &mut Connection, name: &str) -> Result<()> {
    let profile = db::get_project(conn, name)?;
    let mut st = ProjectEditState {
        name: name.to_string(),
        goal: profile.as_ref().and_then(|p| p.goal.clone()),
        stack: profile.as_ref().and_then(|p| p.stack.clone()),
        conventions: profile.as_ref().and_then(|p| p.conventions.clone()),
        notes: profile.as_ref().and_then(|p| p.notes.clone()),
        path: profile.as_ref().and_then(|p| p.path.clone()),
        task_count: db::count_project_tasks(conn, name)?,
        selected: 0,
        editing: false,
        editor: TextArea::default(),
        status: None,
    };

    let mut terminal = crate::infrastructure::tui::init_terminal()?;
    let res = render::edit_loop(&mut terminal, conn, &mut st);
    crate::infrastructure::tui::restore_terminal()?;
    res
}

/// Persist one edited field. The Name field is a true rename (cascades to every
/// task); the rest update the profile via `save_project_profile`.
pub(super) fn apply_edit(
    conn: &mut Connection,
    st: &mut ProjectEditState,
    field: ProjField,
    value: &str,
) -> Result<()> {
    match field {
        ProjField::Name => {
            let new = value.trim();
            if new == st.name {
                return Ok(());
            }
            match db::rename_project(conn, &st.name, new) {
                Ok(n) => {
                    st.name = new.to_string();
                    st.status = Some(format!("Renamed → '{new}' ({n} task(s) moved)"));
                }
                Err(e) => st.status = Some(format!("Rename failed: {e}")),
            }
        }
        other => {
            // Store the literal value (empty clears the field to "").
            let v = Some(value.to_string());
            match other {
                ProjField::Goal => st.goal = v,
                ProjField::Stack => st.stack = v,
                ProjField::Conventions => st.conventions = v,
                ProjField::Notes => st.notes = v,
                ProjField::Name => unreachable!(),
            }
            db::save_project_profile(conn, &st.to_profile())?;
            st.status = Some(format!("Saved {}", other.label().to_lowercase()));
        }
    }
    Ok(())
}

/// Collapse a possibly multi-line value to a single display line.
pub(super) fn display_value(s: &str) -> String {
    let flat = s.replace(['\n', '\r'], " ");
    let trimmed = flat.trim();
    if trimmed.is_empty() {
        "—".to_string()
    } else {
        truncate(trimmed, 60)
    }
}

// ── delete ───────────────────────────────────────────────────────────────────

/// Confirm (by retyping the name) then nuke the project and all of its tasks.
/// Runs in normal terminal mode — the browser restores the terminal before
/// dispatching here — mirroring `sara reset`.
fn confirm_and_delete(conn: &mut Connection, name: &str) -> Result<()> {
    let task_count = db::count_project_tasks(conn, name)?;
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
    let deleted = db::reset_project(conn, name)?;
    println!("✔ Deleted project '{name}': removed {deleted} task(s) and its profile.");
    Ok(())
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

    #[test]
    fn display_value_collapses_blanks_newlines_and_truncates() {
        assert_eq!(display_value(""), "—");
        assert_eq!(display_value("   "), "—");
        assert_eq!(display_value("one\ntwo"), "one two");
        assert_eq!(display_value("a\r\nb"), "a  b");
        assert_eq!(display_value(&"x".repeat(80)).chars().count(), 60);
    }

    fn edit_state() -> ProjectEditState {
        ProjectEditState {
            name: "demo".into(),
            goal: Some("ship it".into()),
            stack: None,
            conventions: None,
            notes: None,
            path: Some("/tmp/demo".into()),
            task_count: 0,
            selected: 0,
            editing: false,
            editor: TextArea::default(),
            status: None,
        }
    }

    #[test]
    fn field_value_reads_each_field() {
        let mut st = edit_state();
        st.stack = Some("rust".into());
        assert_eq!(st.field_value(ProjField::Name), "demo");
        assert_eq!(st.field_value(ProjField::Goal), "ship it");
        assert_eq!(st.field_value(ProjField::Stack), "rust");
        // Unset fields read as empty rather than panicking.
        assert_eq!(st.field_value(ProjField::Notes), "");
    }

    #[test]
    fn to_profile_carries_content_and_path_but_not_github() {
        let st = edit_state();
        let p = st.to_profile();
        assert_eq!(p.name, "demo");
        assert_eq!(p.goal.as_deref(), Some("ship it"));
        assert_eq!(p.path.as_deref(), Some("/tmp/demo"));
        // github/timestamp fields are left None so COALESCE preserves them.
        assert!(p.github_repo.is_none());
        assert!(p.initialized_at.is_none());
    }
}
