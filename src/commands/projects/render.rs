use crate::infrastructure::tui::theme::{Ink, ink};
use anyhow::Result;
use crossterm::event::KeyCode;
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::{ProjectAction, ProjectListState, ProjectRow};
use crate::commands::shared::{rel_time, truncate};

pub(super) fn list_loop<B: Backend<Error: Send + Sync + 'static>>(
    terminal: &mut Terminal<B>,
    st: &mut ProjectListState,
) -> Result<ProjectAction> {
    loop {
        let size = terminal.size()?;
        let viewport = size.height.saturating_sub(3);
        crate::infrastructure::tui::scroll_into_view(&mut st.scroll, st.selected as u16, viewport);

        let lines = build_lines(st);
        terminal.draw(|f| render(f, st, &lines))?;

        let Some(key) = crate::infrastructure::tui::next_key(100)? else {
            continue;
        };

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
    let bg = if is_sel {
        ink(Ink::Select)
    } else {
        ink(Ink::Plain)
    };
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
        let s = Style::default().fg(ink(Ink::Text)).bg(bg);
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
                    .fg(ink(Ink::Accent))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(counts, Style::default().fg(ink(Ink::Muted))),
            Span::styled(format!("   {meta}"), Style::default().fg(ink(Ink::Soft))),
            Span::styled(
                format!("   {activity}"),
                Style::default().fg(ink(Ink::Muted)),
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
                .border_style(Style::default().fg(ink(Ink::Accent))),
        )
        .wrap(Wrap { trim: false })
        .scroll((st.scroll, 0));
    f.render_widget(para, chunks[0]);

    let footer = Paragraph::new(Line::from(Span::styled(
        " j/k navigate  Enter open board  PgDn/PgUp scroll  q quit",
        Style::default().fg(ink(Ink::Muted)),
    )));
    f.render_widget(footer, chunks[1]);
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/projects/render.rs"]
mod tests;
