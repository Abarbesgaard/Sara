use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use rusqlite::Connection;
use std::io::{self, Write};
use std::time::Duration;
use tui_textarea::TextArea;

use chrono::{DateTime, Utc};

use crate::config::Config;
use crate::db;
use crate::model::Project;

enum ProjectAction {
    Quit,
    Open(String),
    Edit(String),
    Delete(String),
}

/// One row in the project browser: a project plus the metadata shown for it.
struct ProjectRow {
    name: String,
    goal: Option<String>,
    stack: Option<String>,
    pending: u32,
    done: u32,
    last_activity: Option<DateTime<Utc>>,
}

struct ProjectListState {
    rows: Vec<ProjectRow>,
    selected: usize,
    scroll: u16,
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

        let mut terminal = crate::tui::init_terminal()?;
        let action = list_loop(&mut terminal, &mut st)?;
        crate::tui::restore_terminal()?;

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

fn list_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    st: &mut ProjectListState,
) -> Result<ProjectAction> {
    loop {
        // Keep the selected row inside the viewport (content height = total -
        // borders - footer). One project per line, so line == selected index.
        let size = terminal.size()?;
        let viewport = size.height.saturating_sub(3);
        let line = st.selected as u16;
        if line < st.scroll {
            st.scroll = line;
        } else if viewport > 0 && line >= st.scroll + viewport {
            st.scroll = line + 1 - viewport;
        }

        let lines = build_lines(st);
        terminal.draw(|f| render(f, st, &lines))?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(ProjectAction::Quit),
            KeyCode::Down | KeyCode::Char('j') => {
                if !st.rows.is_empty() {
                    st.selected = (st.selected + 1).min(st.rows.len() - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                st.selected = st.selected.saturating_sub(1);
            }
            KeyCode::PageDown => st.scroll = st.scroll.saturating_add(10),
            KeyCode::PageUp => st.scroll = st.scroll.saturating_sub(10),
            KeyCode::Enter => {
                if let Some(row) = st.rows.get(st.selected) {
                    return Ok(ProjectAction::Open(row.name.clone()));
                }
            }
            KeyCode::Char('e') => {
                if let Some(row) = st.rows.get(st.selected) {
                    return Ok(ProjectAction::Edit(row.name.clone()));
                }
            }
            KeyCode::Char('d') => {
                if let Some(row) = st.rows.get(st.selected) {
                    return Ok(ProjectAction::Delete(row.name.clone()));
                }
            }
            _ => {}
        }
    }
}

fn build_lines(st: &ProjectListState) -> Vec<Line<'static>> {
    let name_w = st
        .rows
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(4)
        .clamp(4, 28);
    st.rows
        .iter()
        .enumerate()
        .map(|(i, r)| project_line(r, i == st.selected, name_w))
        .collect()
}

fn project_line(r: &ProjectRow, is_sel: bool, name_w: usize) -> Line<'static> {
    let bg = if is_sel { Color::Blue } else { Color::Reset };
    let prefix = if is_sel { " ▶ " } else { "   " };

    let name = format!("{:<width$}", truncate(&r.name, name_w), width = name_w);
    let counts = format!("  {:>3} pending · {:>3} done", r.pending, r.done);

    let mut meta = String::new();
    if let Some(g) = r.goal.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        meta.push_str(&truncate(g, 48));
    }
    if let Some(s) = r.stack.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !meta.is_empty() {
            meta.push_str(" · ");
        }
        meta.push_str(&format!("[{}]", truncate(s, 24)));
    }
    let activity = r.last_activity.map(rel_time).unwrap_or_default();

    if is_sel {
        let s = Style::default().fg(Color::White).bg(bg);
        Line::from(vec![
            Span::styled(format!("{prefix}{name}"), s.add_modifier(Modifier::BOLD)),
            Span::styled(counts, s),
            Span::styled(format!("   {meta}"), s),
            Span::styled(format!("   {activity}"), s),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                format!("{prefix}{name}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(counts, Style::default().fg(Color::DarkGray)),
            Span::styled(format!("   {meta}"), Style::default().fg(Color::Gray)),
            Span::styled(
                format!("   {activity}"),
                Style::default().fg(Color::DarkGray),
            ),
        ])
    }
}

fn render(f: &mut Frame, st: &ProjectListState, lines: &[Line]) {
    let area = f.area();
    let total_pending: u32 = st.rows.iter().map(|r| r.pending).sum();
    let title = format!(" Projects · {} · {} pending ", st.rows.len(), total_pending);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let para = Paragraph::new(lines.to_vec())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false })
        .scroll((st.scroll, 0));
    f.render_widget(para, chunks[0]);

    let footer = Paragraph::new(Line::from(Span::styled(
        " j/k navigate  Enter board  e edit  d delete  PgDn/PgUp scroll  q quit",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(footer, chunks[1]);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Compact relative age, e.g. "just now", "5m ago", "3h ago", "2d ago".
fn rel_time(dt: DateTime<Utc>) -> String {
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
enum ProjField {
    Name,
    Goal,
    Stack,
    Conventions,
    Notes,
}

const PROJ_FIELDS: [ProjField; 5] = [
    ProjField::Name,
    ProjField::Goal,
    ProjField::Stack,
    ProjField::Conventions,
    ProjField::Notes,
];

impl ProjField {
    fn label(self) -> &'static str {
        match self {
            ProjField::Name => "Name",
            ProjField::Goal => "Goal",
            ProjField::Stack => "Stack",
            ProjField::Conventions => "Conventions",
            ProjField::Notes => "Notes",
        }
    }
}

struct ProjectEditState {
    /// Name the project currently lives under (updated after a successful rename).
    name: String,
    goal: Option<String>,
    stack: Option<String>,
    conventions: Option<String>,
    notes: Option<String>,
    /// Profile path, preserved across saves (never edited here).
    path: Option<String>,
    task_count: usize,
    selected: usize,
    editing: bool,
    editor: TextArea<'static>,
    /// Transient status / error line shown under the fields.
    status: Option<String>,
}

impl ProjectEditState {
    fn field_value(&self, f: ProjField) -> &str {
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
    fn to_profile(&self) -> Project {
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

    let mut terminal = crate::tui::init_terminal()?;
    let res = edit_loop(&mut terminal, conn, &mut st);
    crate::tui::restore_terminal()?;
    res
}

fn edit_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    conn: &mut Connection,
    st: &mut ProjectEditState,
) -> Result<()> {
    loop {
        let lines = build_edit_lines(st);
        terminal.draw(|f| render_edit(f, st, &lines))?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }

        if st.editing {
            match key.code {
                KeyCode::Enter => {
                    let value = st.editor.lines().join("\n");
                    let field = PROJ_FIELDS[st.selected];
                    apply_edit(conn, st, field, &value)?;
                    st.editing = false;
                }
                KeyCode::Esc => {
                    st.editing = false;
                    st.status = None;
                }
                _ => {
                    st.editor.input(key);
                }
            }
        } else {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => {
                    st.selected = (st.selected + 1).min(PROJ_FIELDS.len() - 1);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    st.selected = st.selected.saturating_sub(1);
                }
                KeyCode::Enter | KeyCode::Char('e') => {
                    let field = PROJ_FIELDS[st.selected];
                    let mut ta = TextArea::default();
                    ta.insert_str(st.field_value(field));
                    st.editor = ta;
                    st.editing = true;
                    st.status = None;
                }
                _ => {}
            }
        }
    }
}

/// Persist one edited field. The Name field is a true rename (cascades to every
/// task); the rest update the profile via `save_project_profile`.
fn apply_edit(
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
fn display_value(s: &str) -> String {
    let flat = s.replace(['\n', '\r'], " ");
    let trimmed = flat.trim();
    if trimmed.is_empty() {
        "—".to_string()
    } else {
        truncate(trimmed, 60)
    }
}

fn build_edit_lines(st: &ProjectEditState) -> Vec<Line<'static>> {
    PROJ_FIELDS
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let is_sel = i == st.selected;
            let prefix = if is_sel { " ▶ " } else { "   " };
            let label = format!("{prefix}{:<12}", f.label());
            let value = display_value(st.field_value(*f));
            if is_sel {
                Line::from(vec![
                    Span::styled(
                        label,
                        Style::default()
                            .fg(Color::White)
                            .bg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" {value}"),
                        Style::default().fg(Color::White).bg(Color::Blue),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        label,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!(" {value}"), Style::default().fg(Color::Gray)),
                ])
            }
        })
        .collect()
}

fn render_edit(f: &mut Frame, st: &ProjectEditState, lines: &[Line]) {
    let area = f.area();
    let title = format!(" Edit project · {} · {} task(s) ", st.name, st.task_count);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if st.editing {
            [
                Constraint::Min(1),
                Constraint::Length(5),
                Constraint::Length(1),
            ]
        } else {
            [
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
        })
        .split(area);

    let fields = Paragraph::new(lines.to_vec())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(fields, chunks[0]);

    if st.editing {
        let field = PROJ_FIELDS[st.selected];
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " Editing {}  (Enter save · Esc cancel) ",
                field.label()
            ))
            .border_style(Style::default().fg(Color::Yellow));
        let inner = block.inner(chunks[1]);
        f.render_widget(block, chunks[1]);
        f.render_widget(&st.editor, inner);
    } else {
        let status = st.status.clone().unwrap_or_default();
        let status_line = Paragraph::new(Line::from(Span::styled(
            format!(" {status}"),
            Style::default().fg(Color::Green),
        )));
        f.render_widget(status_line, chunks[1]);
    }

    let footer = Paragraph::new(Line::from(Span::styled(
        " j/k field  Enter/e edit  Esc back  (Name edits rename & cascade to tasks)",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(footer, chunks[2]);
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
