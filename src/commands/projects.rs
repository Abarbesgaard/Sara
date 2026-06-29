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
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;

enum ProjectAction {
    Quit,
    Open(String),
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
        let action = list_loop(&mut terminal, &mut st)?;
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
        " j/k navigate  Enter open board  PgDn/PgUp scroll  q quit",
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
