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

use crate::infrastructure::model::{Priority, Status, Task};
use crate::infrastructure::tui;
use crate::infrastructure::tui::keymap::{self, Action, KeyDispatcher, Mode};

use super::{BoardAction, BoardState, Feature};

pub(super) fn board_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    st: &mut BoardState,
) -> Result<BoardAction> {
    let mut dispatcher = KeyDispatcher::new();
    let mut showing_help = false;
    loop {
        // Keep the selected row inside the viewport (content height = total - borders - footer).
        let size = terminal.size()?;
        let viewport = size.height.saturating_sub(3);
        let (lines, task_line) = build_lines(st);
        if let Some(&line) = task_line.get(st.selected) {
            if line < st.scroll {
                st.scroll = line;
            } else if viewport > 0 && line >= st.scroll + viewport {
                st.scroll = line + 1 - viewport;
            }
        }

        terminal.draw(|f| {
            render(f, st, &lines);
            if showing_help {
                tui::render_help_overlay(f, "Board", &help_bindings());
            }
        })?;

        if !event::poll(std::time::Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }

        // Any key dismisses the overlay without otherwise acting on it.
        if showing_help {
            showing_help = false;
            continue;
        }

        match dispatcher.dispatch(key, Mode::Normal) {
            Action::Quit => return Ok(BoardAction::Quit),
            Action::Down => {
                if !st.tasks.is_empty() {
                    st.selected = (st.selected + 1).min(st.tasks.len() - 1);
                }
            }
            Action::Up => {
                st.selected = st.selected.saturating_sub(1);
            }
            Action::Top => st.selected = 0,
            Action::Bottom => {
                if !st.tasks.is_empty() {
                    st.selected = st.tasks.len() - 1;
                }
            }
            Action::PageDown => st.scroll = st.scroll.saturating_add(10),
            Action::PageUp => st.scroll = st.scroll.saturating_sub(10),
            Action::Confirm => {
                if let Some(task) = st.tasks.get(st.selected) {
                    return Ok(BoardAction::OpenTask(task.uuid.to_string()));
                }
            }
            Action::Raw(k) if k.code == KeyCode::Char('?') => {
                showing_help = true;
            }
            _ => {}
        }
    }
}

/// Board's help overlay: the shared bindings it actually acts on (no
/// reorder/save/toggle — board has nothing to reorder, save, or check off)
/// plus '?' itself.
fn help_bindings() -> Vec<(&'static str, &'static str)> {
    use keymap::help::*;
    vec![MOVE, TOP_BOTTOM, PAGE, CONFIRM, QUIT, HELP]
}

/// Build the rendered lines and a map from task index -> its line number, so the
/// scroll math and the renderer agree on layout.
fn build_lines(st: &BoardState) -> (Vec<Line<'static>>, Vec<u16>) {
    let mut lines: Vec<Line> = Vec::new();
    let mut task_line: Vec<u16> = vec![0; st.tasks.len()];
    let mut prev_feature: Option<usize> = None;

    for (idx, task) in st.tasks.iter().enumerate() {
        let fi = st.feature_of[idx];
        if prev_feature != Some(fi) {
            if prev_feature.is_some() {
                lines.push(Line::from(""));
            }
            let feat = &st.features[fi];
            lines.push(feature_header(feat));
            prev_feature = Some(fi);
        }

        task_line[idx] = lines.len() as u16;
        let is_sel = idx == st.selected;
        let grouped = st.features[fi].grouped;
        lines.push(task_line_for(task, is_sel, grouped));
    }
    (lines, task_line)
}

fn feature_header(feat: &Feature) -> Line<'static> {
    let complete = feat.total > 0 && feat.done == feat.total;
    let icon = if !feat.grouped {
        "•"
    } else if complete {
        "✓"
    } else {
        "▸"
    };
    let title_color = if complete {
        Color::Green
    } else if feat.grouped {
        Color::Cyan
    } else {
        Color::Gray
    };
    Line::from(vec![
        Span::styled(
            format!(" {icon} {}  ", feat.title),
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{}/{} done", feat.done, feat.total),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

fn task_line_for(task: &Task, is_sel: bool, grouped: bool) -> Line<'static> {
    let bg = if is_sel { Color::Blue } else { Color::Reset };
    let prefix = if is_sel { " ▶ " } else { "   " };
    // Chain connector for tasks that belong to a feature.
    let connector = if grouped { "└ " } else { "" };
    let id_str = task
        .id
        .map(|i| format!("{i:>3}"))
        .unwrap_or_else(|| "  -".to_string());

    if task.status == Status::Completed {
        let base = Style::default()
            .fg(if is_sel {
                Color::White
            } else {
                Color::DarkGray
            })
            .bg(bg);
        Line::from(vec![
            Span::styled(format!("{prefix}{connector}"), base),
            Span::styled(format!("{id_str}  "), base),
            Span::styled(
                task.description.clone(),
                base.add_modifier(Modifier::CROSSED_OUT),
            ),
        ])
    } else {
        let pri_str = task.priority.as_ref().map(|p| p.label()).unwrap_or("-");
        let pri_color = match &task.priority {
            Some(Priority::H) => Color::Red,
            Some(Priority::M) => Color::Yellow,
            Some(Priority::L) => Color::Green,
            None => Color::DarkGray,
        };
        let (meta_style, id_style, pri_style, desc_style) = if is_sel {
            let s = Style::default().fg(Color::White).bg(bg);
            (s, s, s, s.add_modifier(Modifier::BOLD))
        } else {
            (
                Style::default().fg(Color::Gray),
                Style::default().fg(Color::Cyan),
                Style::default().fg(pri_color),
                Style::default(),
            )
        };
        Line::from(vec![
            Span::styled(format!("{prefix}{connector}"), meta_style),
            Span::styled(format!("{id_str}  "), id_style),
            Span::styled(format!("{pri_str:<4}  "), pri_style),
            Span::styled(task.description.clone(), desc_style),
        ])
    }
}

fn render(f: &mut Frame, st: &BoardState, lines: &[Line]) {
    let area = f.area();
    let title = format!(
        " {} · {} feature{} · {} pending, {} done ",
        st.project,
        st.feature_count,
        if st.feature_count == 1 { "" } else { "s" },
        st.pending,
        st.done,
    );

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
        " j/k navigate  gg/G top/bottom  Enter open  PgDn/PgUp scroll  ? help  q quit",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(footer, chunks[1]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn help_bindings_are_all_things_this_screen_actually_handles() {
        // Every listed key must correspond to an Action arm board_loop's
        // match actually acts on (not a shared-vocab no-op like reorder).
        let bindings = help_bindings();
        let labels: Vec<&str> = bindings.iter().map(|(k, _)| *k).collect();
        assert!(labels.contains(&"j/k, ↓/↑"));
        assert!(labels.contains(&"gg / G"));
        assert!(labels.contains(&"Enter"));
        assert!(labels.contains(&"q / Esc"));
        assert!(labels.contains(&"?"));
        // Board has nothing to reorder or save — these would be silent
        // no-ops here, so they must not be listed.
        assert!(!labels.iter().any(|l| l.contains("Ctrl+S")));
        assert!(!labels.iter().any(|l| l.contains("Shift")));
    }

    #[test]
    fn help_overlay_renders_without_panicking() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|f| tui::render_help_overlay(f, "Board", &help_bindings()))
            .unwrap();
        let buf = terminal.backend().buffer();
        let area = *buf.area();
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(out.contains("Board"));
        assert!(out.contains("any key closes"));
    }
}
