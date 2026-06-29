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
use std::time::Duration;

use super::{ProjectAction, ProjectListState, ProjectRow};

pub(super) fn list_loop<B: Backend>(
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

    let name = format!(
        "{:<width$}",
        super::truncate(&r.name, name_w),
        width = name_w
    );
    let counts = format!("  {:>3} pending · {:>3} done", r.pending, r.done);

    let mut meta = String::new();
    if let Some(g) = r.goal.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        meta.push_str(&super::truncate(g, 48));
    }
    if let Some(s) = r.stack.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !meta.is_empty() {
            meta.push_str(" · ");
        }
        meta.push_str(&format!("[{}]", super::truncate(s, 24)));
    }
    let activity = r.last_activity.map(super::rel_time).unwrap_or_default();

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
