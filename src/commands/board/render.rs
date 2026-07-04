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

use crate::infrastructure::db::LinkFlags;
use crate::infrastructure::model::{Priority, Status, Task};
use crate::infrastructure::tui;
use crate::infrastructure::tui::keymap::{self, Action, KeyDispatcher, Mode};

use super::{BoardAction, BoardState, IssueNode};

/// One selectable row in the flattened, expansion-aware tree.
#[derive(Clone, Copy)]
pub(super) enum Row {
    Issue(usize),
    Task(usize, usize),
    Standalone(usize),
}

/// Flatten the tree into the rows currently on screen: every issue header,
/// plus its child tasks only when expanded, followed by standalone tasks.
pub(super) fn visible_rows(st: &BoardState) -> Vec<Row> {
    let mut rows = Vec::new();
    for (gi, issue) in st.issues.iter().enumerate() {
        rows.push(Row::Issue(gi));
        if issue.expanded {
            for ti in 0..issue.tasks.len() {
                rows.push(Row::Task(gi, ti));
            }
        }
    }
    for si in 0..st.standalone.len() {
        rows.push(Row::Standalone(si));
    }
    rows
}

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
        let rows = visible_rows(st);
        let (lines, row_line) = build_lines(st, &rows);
        if let Some(&line) = row_line.get(st.selected) {
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
                if !rows.is_empty() {
                    st.selected = (st.selected + 1).min(rows.len() - 1);
                }
            }
            Action::Up => {
                st.selected = st.selected.saturating_sub(1);
            }
            Action::Top => st.selected = 0,
            Action::Bottom => {
                if !rows.is_empty() {
                    st.selected = rows.len() - 1;
                }
            }
            Action::PageDown => st.scroll = st.scroll.saturating_add(10),
            Action::PageUp => st.scroll = st.scroll.saturating_sub(10),
            // Space — neotree-style toggle of the selected issue node.
            Action::ToggleMark => {
                if let Some(Row::Issue(gi)) = rows.get(st.selected).copied() {
                    st.issues[gi].expanded = !st.issues[gi].expanded;
                }
            }
            Action::Confirm => match rows.get(st.selected).copied() {
                Some(Row::Issue(gi)) => st.issues[gi].expanded = !st.issues[gi].expanded,
                Some(Row::Task(gi, ti)) => {
                    let task = &st.issues[gi].tasks[ti];
                    return Ok(BoardAction::OpenTask(task.uuid.to_string()));
                }
                Some(Row::Standalone(si)) => {
                    let task = &st.standalone[si];
                    return Ok(BoardAction::OpenTask(task.uuid.to_string()));
                }
                None => {}
            },
            // 'o' — neotree-familiar toggle synonym for Space/Enter on a node.
            Action::Raw(k) if k.code == KeyCode::Char('o') => {
                if let Some(Row::Issue(gi)) = rows.get(st.selected).copied() {
                    st.issues[gi].expanded = !st.issues[gi].expanded;
                }
            }
            Action::Raw(k) if k.code == KeyCode::Right || k.code == KeyCode::Char('l') => {
                if let Some(Row::Issue(gi)) = rows.get(st.selected).copied() {
                    st.issues[gi].expanded = true;
                }
            }
            Action::Raw(k) if k.code == KeyCode::Left || k.code == KeyCode::Char('h') => {
                match rows.get(st.selected).copied() {
                    Some(Row::Issue(gi)) => st.issues[gi].expanded = false,
                    Some(Row::Task(gi, _)) => {
                        // Collapse the parent and land the cursor back on its header.
                        st.issues[gi].expanded = false;
                        if let Some(pos) =
                            rows.iter().position(|r| matches!(r, Row::Issue(i) if *i == gi))
                        {
                            st.selected = pos;
                        }
                    }
                    _ => {}
                }
            }
            Action::Raw(k) if k.code == KeyCode::Char('?') => {
                showing_help = true;
            }
            _ => {}
        }
    }
}

/// Board's help overlay: the shared bindings it actually acts on, plus its
/// own tree expand/collapse keys and '?' itself.
fn help_bindings() -> Vec<(&'static str, &'static str)> {
    use keymap::help::*;
    vec![
        MOVE,
        TOP_BOTTOM,
        PAGE,
        ("o / Space / Enter", "expand / collapse an issue"),
        ("l / →", "expand an issue"),
        ("h / ←", "collapse an issue (or its parent)"),
        CONFIRM,
        QUIT,
        HELP,
    ]
}

/// Build the rendered lines and a map from row index -> its line number, so the
/// scroll math and the renderer agree on layout.
fn build_lines(st: &BoardState, rows: &[Row]) -> (Vec<Line<'static>>, Vec<u16>) {
    let mut lines: Vec<Line> = Vec::with_capacity(rows.len());
    let mut row_line: Vec<u16> = vec![0; rows.len()];

    for (i, row) in rows.iter().enumerate() {
        row_line[i] = lines.len() as u16;
        let is_sel = i == st.selected;
        match *row {
            Row::Issue(gi) => lines.push(issue_header(&st.issues[gi], is_sel)),
            Row::Task(gi, ti) => {
                let task = &st.issues[gi].tasks[ti];
                lines.push(task_line_for(task, is_sel, true, badge_for(st, task)));
            }
            Row::Standalone(si) => {
                let task = &st.standalone[si];
                lines.push(task_line_for(task, is_sel, false, badge_for(st, task)));
            }
        }
    }
    (lines, row_line)
}

fn badge_for(st: &BoardState, task: &Task) -> Option<Span<'static>> {
    let uuid = task.uuid.to_string();
    let flags = st.badges.get(&uuid).copied().unwrap_or_default();
    let synced = st.imported.contains(&uuid);
    board_badge_span(flags, synced)
}

/// Badge span for a task's PR/issue links, mirroring `sara list`'s badge
/// precedence (PR > issue > generic link). `synced` gates the ISS badge to
/// the task that *is* the `sara sync`-imported issue — every task nested
/// under an issue header already links back to it for traceability, so a raw
/// `flags.issue` check would tag all of them, not just the imported one.
/// Kept local rather than reusing `commands::list`'s private `LinkBadge`
/// type — command slices don't import each other; only the underlying data
/// (`link_flags_by_task` / `github_synced_task_uuids`) is shared.
fn board_badge_span(flags: LinkFlags, synced: bool) -> Option<Span<'static>> {
    if flags.pr {
        Some(Span::styled(
            "PR ",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ))
    } else if flags.issue && synced {
        Some(Span::styled(
            "ISS ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ))
    } else if flags.any {
        Some(Span::styled("↗ ", Style::default().fg(Color::Cyan)))
    } else {
        None
    }
}

fn issue_header(issue: &IssueNode, is_sel: bool) -> Line<'static> {
    let total = issue.total;
    let done = issue.done;
    let complete = total > 0 && done == total;
    let icon = if issue.expanded { "▾" } else { "▸" };
    let bg = if is_sel { Color::Blue } else { Color::Reset };
    let title_color = if is_sel {
        Color::White
    } else if complete {
        Color::Green
    } else {
        Color::Cyan
    };

    let label = match &issue.title {
        Some(t) => format!(
            " {icon} #{} {}  {}  ",
            issue.number,
            issue.owner_repo,
            truncate(t, 50)
        ),
        None => format!(" {icon} #{} {}  ", issue.number, issue.owner_repo),
    };

    Line::from(vec![
        Span::styled(
            label,
            Style::default()
                .fg(title_color)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{done}/{total} done"),
            Style::default()
                .fg(if is_sel { Color::White } else { Color::DarkGray })
                .bg(bg),
        ),
    ])
}

fn task_line_for(
    task: &Task,
    is_sel: bool,
    nested: bool,
    badge_span: Option<Span<'static>>,
) -> Line<'static> {
    let bg = if is_sel { Color::Blue } else { Color::Reset };
    let prefix = if is_sel { " ▶ " } else { "   " };
    // Tree connector for tasks nested under an issue header.
    let connector = if nested { "└ " } else { "" };
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
        let mut spans = vec![
            Span::styled(format!("{prefix}{connector}"), base),
            Span::styled(format!("{id_str}  "), base),
        ];
        spans.extend(badge_span);
        spans.push(Span::styled(
            task.description.clone(),
            base.add_modifier(Modifier::CROSSED_OUT),
        ));
        Line::from(spans)
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
        let mut spans = vec![
            Span::styled(format!("{prefix}{connector}"), meta_style),
            Span::styled(format!("{id_str}  "), id_style),
            Span::styled(format!("{pri_str:<4}  "), pri_style),
        ];
        spans.extend(badge_span);
        spans.push(Span::styled(task.description.clone(), desc_style));
        Line::from(spans)
    }
}

fn render(f: &mut Frame, st: &BoardState, lines: &[Line]) {
    let area = f.area();
    let issue_count = st.issues.len();
    let title = format!(
        " {} · {} issue{} · {} pending, {} done ",
        st.project,
        issue_count,
        if issue_count == 1 { "" } else { "s" },
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
        " j/k navigate  o/Space/Enter toggle  h/l collapse/expand  ? help  q quit",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::model::Task;
    use ratatui::{Terminal, backend::TestBackend};
    use std::collections::{HashMap, HashSet};

    fn board_state(issues: Vec<IssueNode>, standalone: Vec<Task>) -> BoardState {
        let pending = issues
            .iter()
            .flat_map(|i| &i.tasks)
            .chain(standalone.iter())
            .filter(|t| t.status != Status::Completed)
            .count();
        BoardState {
            project: "tk".to_string(),
            pending,
            done: 0,
            issues,
            standalone,
            badges: HashMap::new(),
            show_finished: false,
            imported: HashSet::new(),
            selected: 0,
            scroll: 0,
        }
    }

    fn issue_node(number: u64, tasks: Vec<Task>, expanded: bool) -> IssueNode {
        let total = tasks.len();
        let done = tasks.iter().filter(|t| t.status == Status::Completed).count();
        IssueNode {
            owner_repo: "o/r".to_string(),
            number,
            title: None,
            tasks,
            done,
            total,
            expanded,
        }
    }

    fn draw(st: &BoardState) -> String {
        let rows = visible_rows(st);
        let (lines, _) = build_lines(st, &rows);
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal.draw(|f| render(f, st, &lines)).unwrap();
        let buf = terminal.backend().buffer();
        let area = *buf.area();
        let mut out = String::new();
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }

    #[test]
    fn collapsed_issue_hides_its_tasks() {
        let t = Task::new("child".into(), "tk".into());
        let st = board_state(vec![issue_node(5, vec![t], false)], vec![]);
        let out = draw(&st);
        assert!(out.contains("#5"));
        assert!(!out.contains("child"));
    }

    #[test]
    fn expanded_issue_shows_its_tasks() {
        let t = Task::new("child".into(), "tk".into());
        let st = board_state(vec![issue_node(5, vec![t], true)], vec![]);
        let out = draw(&st);
        assert!(out.contains("#5"));
        assert!(out.contains("child"));
    }

    #[test]
    fn pr_badge_renders_on_the_row() {
        let t = Task::new("has a pr".into(), "tk".into());
        let uuid = t.uuid.to_string();
        let mut st = board_state(vec![issue_node(5, vec![t], true)], vec![]);
        st.badges.insert(
            uuid,
            LinkFlags {
                any: true,
                pr: true,
                issue: false,
            },
        );
        assert!(draw(&st).contains("PR"));
    }

    #[test]
    fn issue_badge_only_renders_for_synced_tasks() {
        let t = Task::new("has an issue".into(), "tk".into());
        let uuid = t.uuid.to_string();
        let mut st = board_state(vec![issue_node(5, vec![t], true)], vec![]);
        st.badges.insert(
            uuid.clone(),
            LinkFlags {
                any: true,
                pr: false,
                issue: true,
            },
        );
        // Not synced yet — every task under an issue header links back to
        // it for traceability, so the badge must not fire on that alone.
        assert!(!draw(&st).contains("ISS"));

        st.imported.insert(uuid);
        assert!(draw(&st).contains("ISS"));
    }

    #[test]
    fn no_badge_for_task_without_links() {
        let t = Task::new("plain".into(), "tk".into());
        let st = board_state(vec![issue_node(5, vec![t], true)], vec![]);
        let out = draw(&st);
        assert!(!out.contains("PR"));
        assert!(!out.contains("ISS"));
    }

    #[test]
    fn standalone_tasks_render_without_an_issue_header() {
        let t = Task::new("loose".into(), "tk".into());
        let st = board_state(vec![], vec![t]);
        let out = draw(&st);
        assert!(out.contains("loose"));
        assert!(!out.contains('#'));
    }

    #[test]
    fn title_reports_issue_count() {
        let t = Task::new("x".into(), "tk".into());
        let st = board_state(vec![issue_node(5, vec![t], false)], vec![]);
        assert!(draw(&st).contains("1 issue"));
    }

    #[test]
    fn help_bindings_are_all_things_this_screen_actually_handles() {
        let bindings = help_bindings();
        let labels: Vec<&str> = bindings.iter().map(|(k, _)| *k).collect();
        assert!(labels.contains(&"j/k, ↓/↑"));
        assert!(labels.contains(&"gg / G"));
        assert!(labels.contains(&"Enter"));
        assert!(labels.contains(&"q / Esc"));
        assert!(labels.contains(&"?"));
        let descriptions: Vec<&str> = bindings.iter().map(|(_, d)| *d).collect();
        assert!(descriptions.iter().any(|d| d.contains("expand")));
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
