use crate::infrastructure::tui::theme::{Ink, ink};
use anyhow::Result;
use chrono::{DateTime, Utc};
use crossterm::event::KeyCode;
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use crate::infrastructure::db::LinkFlags;
use crate::infrastructure::model::{Priority, Status, Task};
use crate::infrastructure::tui;
use crate::infrastructure::tui::keymap::{self, Action, KeyDispatcher, Mode};

use super::types::{CardInfo, Freshness};
use super::{BoardAction, BoardState, IssueNode};
use crate::commands::shared::truncate;

const FIXED_OVERHEAD: u16 = 10;

#[derive(Clone, Copy)]
pub(super) enum Row {
    Issue(usize),
    Task(usize, usize),
    Standalone(usize),
}

pub(super) fn matches_filter(task: &Task, filter: &str) -> bool {
    let desc = task.description.to_lowercase();
    filter.split_whitespace().all(|term| {
        let term = term.to_lowercase();
        match term.strip_prefix('+') {
            Some(tag) => task.tags.iter().any(|t| t.to_lowercase() == tag),
            None => {
                desc.contains(&term) || task.tags.iter().any(|t| t.to_lowercase().contains(&term))
            }
        }
    })
}

pub(super) fn visible_rows(st: &BoardState) -> Vec<Row> {
    let filtering = !st.filter.trim().is_empty();
    let mut rows = Vec::new();
    for (gi, issue) in st.issues.iter().enumerate() {
        let hits: Vec<usize> = (0..issue.tasks.len())
            .filter(|&ti| !filtering || matches_filter(&issue.tasks[ti], &st.filter))
            .collect();
        if filtering && hits.is_empty() {
            continue;
        }
        rows.push(Row::Issue(gi));
        if issue.expanded || filtering {
            rows.extend(hits.into_iter().map(|ti| Row::Task(gi, ti)));
        }
    }
    for (si, t) in st.standalone.iter().enumerate() {
        if !filtering || matches_filter(t, &st.filter) {
            rows.push(Row::Standalone(si));
        }
    }
    rows
}

pub(super) fn next_pick(st: &BoardState) -> Option<&Task> {
    st.issues
        .iter()
        .flat_map(|i| &i.tasks)
        .chain(st.standalone.iter())
        .filter(|t| t.status == Status::Pending)
        .max_by(|a, b| {
            a.urgency
                .partial_cmp(&b.urgency)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn selected_uuid(st: &BoardState, rows: &[Row]) -> Option<String> {
    match rows.get(st.selected).copied()? {
        Row::Task(gi, ti) => Some(st.issues[gi].tasks[ti].uuid.to_string()),
        Row::Standalone(si) => Some(st.standalone[si].uuid.to_string()),
        Row::Issue(_) => None,
    }
}

pub(super) const PREVIEW_MIN_WIDTH: u16 = 110;

pub(super) fn preview_width(total: u16) -> Option<u16> {
    (total >= PREVIEW_MIN_WIDTH).then(|| (total * 45 / 100).saturating_sub(2))
}

pub(super) type PreviewLoader<'a> = dyn FnMut(&str, u16) -> Vec<Line<'static>> + 'a;

pub(super) fn board_loop<B: Backend<Error: Send + Sync + 'static>>(
    terminal: &mut Terminal<B>,
    st: &mut BoardState,
    load_preview: &mut PreviewLoader,
) -> Result<BoardAction> {
    let mut dispatcher = KeyDispatcher::new();
    let mut showing_help = false;
    let mut cache: std::collections::HashMap<(String, u16), Vec<Line<'static>>> =
        std::collections::HashMap::new();
    loop {
        let size = terminal.size()?;
        let viewport = size.height.saturating_sub(FIXED_OVERHEAD);
        let rows = visible_rows(st);
        let (lines, row_line) = build_lines(st, &rows);
        if let Some(&line) = row_line.get(st.selected) {
            let tail = if matches!(rows.get(st.selected), Some(Row::Issue(_))) {
                line
            } else {
                line + 1
            };
            crate::infrastructure::tui::scroll_into_view(&mut st.scroll, tail, viewport);
            crate::infrastructure::tui::scroll_into_view(&mut st.scroll, line, viewport);
        }

        let pane = match (st.preview, preview_width(size.width)) {
            (true, Some(w)) => Some(match selected_uuid(st, &rows) {
                Some(uuid) => cache
                    .entry((uuid.clone(), w))
                    .or_insert_with(|| load_preview(&uuid, w))
                    .clone(),
                None => vec![],
            }),
            _ => None,
        };

        terminal.draw(|f| {
            render_with(f, st, &lines, pane.as_deref());
            if showing_help {
                tui::render_help_overlay(f, "Board", &help_bindings());
            }
        })?;

        let Some(key) = crate::infrastructure::tui::next_key(100)? else {
            continue;
        };

        if showing_help {
            showing_help = false;
            continue;
        }

        if st.filtering {
            match key.code {
                KeyCode::Esc => {
                    st.filter.clear();
                    st.filtering = false;
                }
                KeyCode::Enter => st.filtering = false,
                KeyCode::Backspace => {
                    st.filter.pop();
                }
                KeyCode::Char(c) => st.filter.push(c),
                _ => {}
            }
            st.selected = 0;
            st.scroll = 0;
            continue;
        }

        if key.code == KeyCode::Esc && !st.filter.is_empty() {
            st.filter.clear();
            st.selected = 0;
            st.scroll = 0;
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
                        st.issues[gi].expanded = false;
                        if let Some(pos) = rows
                            .iter()
                            .position(|r| matches!(r, Row::Issue(i) if *i == gi))
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
            Action::Raw(k) if k.code == KeyCode::Char('/') => {
                st.filtering = true;
            }
            Action::Raw(k) if k.code == KeyCode::Char('p') => {
                st.preview = !st.preview;
            }
            _ => {}
        }
    }
}

fn help_bindings() -> Vec<(&'static str, &'static str)> {
    use keymap::help::*;
    vec![
        MOVE,
        TOP_BOTTOM,
        PAGE,
        ("o / Space / Enter", "expand / collapse an issue"),
        ("l / →", "expand an issue"),
        ("h / ←", "collapse an issue (or its parent)"),
        ("/", "filter by text or +tag (Esc clears)"),
        ("p", "toggle the task preview pane"),
        CONFIRM,
        QUIT,
        HELP,
    ]
}

fn build_lines(st: &BoardState, rows: &[Row]) -> (Vec<Line<'static>>, Vec<u16>) {
    let next = next_pick(st).map(|t| t.uuid);
    let mut lines: Vec<Line> = Vec::with_capacity(rows.len() * 2);
    let mut row_line: Vec<u16> = vec![0; rows.len()];

    for (i, row) in rows.iter().enumerate() {
        row_line[i] = lines.len() as u16;
        let is_sel = i == st.selected;
        match *row {
            Row::Issue(gi) => lines.push(issue_header(&st.issues[gi], is_sel)),
            Row::Task(gi, ti) => {
                let issue = &st.issues[gi];
                let task = &issue.tasks[ti];
                let last = !matches!(rows.get(i + 1), Some(Row::Task(g, _)) if *g == gi);
                let connector = if last { "└─" } else { "├─" };
                lines.push(task_line_for(
                    task,
                    is_sel,
                    Some(connector),
                    badge_for(st, task),
                    next == Some(task.uuid),
                ));
                lines.push(card_strip(st, task, is_sel, Some(!last)));
            }
            Row::Standalone(si) => {
                let task = &st.standalone[si];
                lines.push(task_line_for(
                    task,
                    is_sel,
                    None,
                    badge_for(st, task),
                    next == Some(task.uuid),
                ));
                lines.push(card_strip(st, task, is_sel, None));
            }
        }
    }
    (lines, row_line)
}

const STRIP_INDENT: usize = 1 + TREE_W + ID_W + 2 + PRI_W + 1 + AGE_W + 2 + BADGE_W;

pub(super) fn card_strip(
    st: &BoardState,
    task: &Task,
    is_sel: bool,
    rail: Option<bool>,
) -> Line<'static> {
    let card = st
        .cards
        .get(&task.uuid.to_string())
        .cloned()
        .unwrap_or_default();
    let bg = if is_sel {
        ink(Ink::Select)
    } else {
        ink(Ink::Plain)
    };
    let done = task.status == Status::Completed;
    let tone = |role: Ink| {
        let fg = if done { ink(Ink::Muted) } else { ink(role) };
        Style::default().fg(fg).bg(bg)
    };
    let lead = match rail {
        Some(true) => format!(" │{}", " ".repeat(STRIP_INDENT - 2)),
        _ => " ".repeat(STRIP_INDENT),
    };
    let mut spans = vec![Span::styled(lead, tone(Ink::Muted))];
    let mut push = |text: String, style: Style| {
        if spans.len() > 1 {
            spans.push(Span::styled("  ", Style::default().bg(bg)));
        }
        spans.push(Span::styled(text, style));
    };
    if next_pick(st).is_some_and(|n| n.uuid == task.uuid) {
        push(
            "★\u{a0}NEXT".to_string(),
            tone(Ink::Accent).add_modifier(Modifier::BOLD),
        );
    }
    for (text, style) in strip_badges(task, &card, done) {
        push(text, tone(style));
    }
    Line::from(spans)
}

pub(super) fn strip_badges(task: &Task, card: &CardInfo, done: bool) -> Vec<(String, Ink)> {
    let mut out = vec![match card.freshness {
        Freshness::Valid => ("✓\u{a0}valid".to_string(), Ink::Ok),
        Freshness::Stale => ("⚠\u{a0}stale".to_string(), Ink::Warn),
        Freshness::Unvalidated => ("·\u{a0}unvalidated".to_string(), Ink::Muted),
    }];
    out.push(if card.accept_total == 0 {
        ("☐\u{a0}no\u{a0}criteria".to_string(), Ink::Muted)
    } else {
        let role = if card.accept_done == card.accept_total {
            Ink::Ok
        } else {
            Ink::Soft
        };
        (
            format!("☐\u{a0}{}/{}", card.accept_done, card.accept_total),
            role,
        )
    });
    out.push(match (card.feedback, card.revise) {
        (0, _) => ("·\u{a0}no\u{a0}feedback".to_string(), Ink::Muted),
        (n, 0) => (format!("!\u{a0}{n}\u{a0}feedback"), Ink::Accent),
        (n, r) => (format!("!\u{a0}{n}\u{a0}⟳\u{a0}{r}\u{a0}revise"), Ink::Warn),
    });
    if !card.blocked_by.is_empty() {
        let ids: Vec<String> = card.blocked_by.iter().map(|i| format!("#{i}")).collect();
        out.push((format!("⊘\u{a0}blocked\u{a0}{}", ids.join(" ")), Ink::Err));
    }
    if !done && let Some(due) = task.due {
        out.push(due_badge(due));
    }
    if let Some(b) = &card.branch {
        out.push((format!("⎇\u{a0}{}", truncate(b, 28)), Ink::Info));
    }
    if !done && let Some(step) = &card.step {
        out.push((format!("▸\u{a0}{}", truncate(step, 40)), Ink::Soft));
    }
    out
}

fn due_badge(due: DateTime<Utc>) -> (String, Ink) {
    let secs = (due - Utc::now()).num_seconds();
    let days = secs.abs() / 86400;
    let hours = secs.abs() / 3600;
    let span = if days >= 1 {
        format!("{days}d")
    } else {
        format!("{hours}h")
    };
    if secs < 0 {
        (format!("◷\u{a0}overdue\u{a0}{span}"), Ink::Err)
    } else if secs < 2 * 86400 {
        (format!("◷\u{a0}due\u{a0}{span}"), Ink::Warn)
    } else {
        (format!("◷\u{a0}due\u{a0}{span}"), Ink::Soft)
    }
}

fn badge_for(st: &BoardState, task: &Task) -> Span<'static> {
    let uuid = task.uuid.to_string();
    let flags = st.badges.get(&uuid).copied().unwrap_or_default();
    let synced = st.imported.contains(&uuid);
    board_badge_span(flags, synced)
}

fn board_badge_span(flags: LinkFlags, synced: bool) -> Span<'static> {
    let (label, color) = if flags.pr {
        ("PR", ink(Ink::Special))
    } else if flags.issue && synced {
        ("ISS", ink(Ink::Ok))
    } else if flags.any {
        ("↗", ink(Ink::Accent))
    } else {
        ("", ink(Ink::Plain))
    };
    let text = format!("{:<w$}", label, w = BADGE_W);
    if label.is_empty() {
        Span::raw(text)
    } else {
        Span::styled(
            text,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )
    }
}

const TREE_W: usize = 3;
const ID_W: usize = 3;
const PRI_W: usize = 3;
const AGE_W: usize = 7;
const BADGE_W: usize = 5;

fn col_header_line() -> Line<'static> {
    let mut s = String::new();
    s.push(' ');
    s.push_str(&" ".repeat(TREE_W));
    s.push_str(&format!("{:>w$}", "ID", w = ID_W));
    s.push_str("  ");
    s.push_str(&format!("{:<w$}", "PRI", w = PRI_W));
    s.push(' ');
    s.push_str(&format!("{:<w$}", "AGE", w = AGE_W));
    s.push_str("  ");
    s.push_str(&" ".repeat(BADGE_W));
    s.push_str("DESCRIPTION");
    Line::from(Span::styled(
        s,
        Style::default()
            .fg(ink(Ink::Text))
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    ))
}

fn age_str(entry: DateTime<Utc>) -> String {
    let secs = (Utc::now() - entry).num_seconds().max(0);
    let days = secs / 86400;
    if days >= 1 {
        format!("{}d", days)
    } else {
        let hours = secs / 3600;
        if hours >= 1 {
            format!("{}h", hours)
        } else {
            format!("{}m", secs / 60)
        }
    }
}

fn priority_chip(pri: Option<&Priority>, is_sel: bool, row_bg: Color) -> Span<'static> {
    let label = match pri {
        Some(Priority::H) => "H",
        Some(Priority::M) => "M",
        Some(Priority::L) => "L",
        None => "-",
    };
    let text = format!("{label:^PRI_W$}");
    if is_sel {
        return Span::styled(
            text,
            Style::default()
                .fg(ink(Ink::Text))
                .bg(row_bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    match pri {
        Some(Priority::H) => Span::styled(
            text,
            Style::default()
                .fg(ink(Ink::Text))
                .bg(ink(Ink::Err))
                .add_modifier(Modifier::BOLD),
        ),
        Some(Priority::M) => Span::styled(
            text,
            Style::default()
                .fg(ink(Ink::Base))
                .bg(ink(Ink::Warn))
                .add_modifier(Modifier::BOLD),
        ),
        Some(Priority::L) => Span::styled(
            text,
            Style::default()
                .fg(ink(Ink::Base))
                .bg(ink(Ink::Ok))
                .add_modifier(Modifier::BOLD),
        ),
        None => Span::styled(text, Style::default().fg(ink(Ink::Muted)).bg(row_bg)),
    }
}

fn issue_header(issue: &IssueNode, is_sel: bool) -> Line<'static> {
    let bg = if is_sel {
        ink(Ink::Select)
    } else {
        ink(Ink::Header)
    };
    let total = issue.total;
    let done = issue.done;
    let complete = total > 0 && done == total;
    let expand_icon = if issue.expanded { "▾" } else { "▸" };

    let max_pri = issue
        .tasks
        .iter()
        .filter_map(|t| t.priority.as_ref())
        .max_by_key(|p| match p {
            Priority::H => 3,
            Priority::M => 2,
            Priority::L => 1,
        })
        .cloned();

    let age = issue
        .tasks
        .iter()
        .map(|t| t.entry)
        .min()
        .map(age_str)
        .unwrap_or_default();

    let title_color = if is_sel {
        ink(Ink::Text)
    } else if complete {
        ink(Ink::Ok)
    } else {
        ink(Ink::Accent)
    };

    let issue_label = match &issue.title {
        Some(t) => format!(
            "#{} {}  {}",
            issue.number,
            issue.owner_repo,
            truncate(t, 45)
        ),
        None => format!("#{} {}", issue.number, issue.owner_repo),
    };

    let meta = if is_sel {
        Style::default().fg(ink(Ink::Text)).bg(bg)
    } else {
        Style::default().fg(ink(Ink::Muted)).bg(bg)
    };

    let mut spans = vec![
        Span::styled(if is_sel { "▶" } else { "▌" }, meta.fg(ink(Ink::Accent))),
        Span::styled(format!("{expand_icon:<w$}", w = TREE_W), meta),
        Span::styled(" ".repeat(ID_W), meta),
        Span::styled("  ", meta),
        priority_chip(max_pri.as_ref(), is_sel, bg),
        Span::styled(" ", meta),
        Span::styled(format!("{age:<w$}", w = AGE_W), meta),
        Span::styled("  ", meta),
        Span::styled(" ".repeat(BADGE_W), meta),
        Span::styled(
            issue_label,
            Style::default()
                .fg(title_color)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", meta),
    ];
    spans.extend(
        tui::screen::bar(done, total, 10)
            .into_iter()
            .map(|sp| sp.patch_style(Style::default().bg(bg))),
    );
    spans.push(Span::styled(format!(" {done}/{total}"), meta));
    Line::from(spans)
}

fn task_line_for(
    task: &Task,
    is_sel: bool,
    connector: Option<&'static str>,
    badge_span: Span<'static>,
    is_next: bool,
) -> Line<'static> {
    let bg = if is_sel {
        ink(Ink::Select)
    } else {
        ink(Ink::Plain)
    };
    let sel_ch = if is_sel { "▶" } else { " " };
    let tree = match connector {
        Some(c) => format!(" {c}"),
        None => " ".repeat(TREE_W),
    };
    let id_str = task
        .id
        .map(|i| format!("{i:>w$}", w = ID_W))
        .unwrap_or_else(|| format!("{:>w$}", "-", w = ID_W));
    let age = age_str(task.entry);

    if task.status == Status::Completed {
        let base = Style::default()
            .fg(if is_sel {
                ink(Ink::Text)
            } else {
                ink(Ink::Muted)
            })
            .bg(bg);
        let spans = vec![
            Span::styled(format!("{sel_ch}{tree}"), base),
            Span::styled(format!("{id_str}  "), base),
            priority_chip(None, is_sel, bg),
            Span::styled(" ", base),
            Span::styled(format!("{age:<w$}  ", w = AGE_W), base),
            badge_span,
            Span::styled(
                task.description.clone(),
                base.add_modifier(Modifier::CROSSED_OUT),
            ),
        ];
        return Line::from(spans);
    }

    let (meta_s, id_s, age_s, desc_s) = if is_sel {
        let s = Style::default().fg(ink(Ink::Text)).bg(bg);
        (s, s, s, s.add_modifier(Modifier::BOLD))
    } else {
        (
            Style::default().fg(ink(Ink::Soft)).bg(bg),
            Style::default().fg(ink(Ink::Accent)).bg(bg),
            Style::default().fg(ink(Ink::Muted)).bg(bg),
            Style::default().bg(bg),
        )
    };
    let mut spans = vec![
        Span::styled(format!("{sel_ch}{tree}"), meta_s),
        Span::styled(format!("{id_str}  "), id_s),
        priority_chip(task.priority.as_ref(), is_sel, bg),
        Span::styled(" ", meta_s),
        Span::styled(format!("{age:<w$}  ", w = AGE_W), age_s),
        badge_span,
        Span::styled(task.description.clone(), desc_s),
    ];
    if is_next && !is_sel {
        spans[0] = Span::styled(
            format!("★{tree}"),
            Style::default()
                .fg(ink(Ink::Accent))
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    Line::from(spans)
}

fn active_count(st: &BoardState) -> usize {
    st.issues
        .iter()
        .flat_map(|i| &i.tasks)
        .chain(st.standalone.iter())
        .filter(|t| t.is_active())
        .count()
}

fn next_task_label(st: &BoardState) -> String {
    match next_pick(st) {
        Some(t) => {
            let id = t.id.map(|i| format!("#{i} ")).unwrap_or_default();
            format!("{}{}", id, truncate(&t.description, 24))
        }
        None => "—".to_string(),
    }
}

fn render_stats(f: &mut Frame, st: &BoardState, area: Rect) {
    let total = st.pending + st.done;
    let active = active_count(st);

    let bracket = Style::default()
        .fg(ink(Ink::Accent))
        .add_modifier(Modifier::BOLD);
    let label = Style::default().fg(ink(Ink::Muted));
    let value = Style::default()
        .fg(ink(Ink::Text))
        .add_modifier(Modifier::BOLD);
    let sep = Style::default().fg(ink(Ink::Muted));

    let left_spans: Vec<Span> = vec![
        Span::styled("[ ", bracket),
        Span::styled("Total: ", label),
        Span::styled(total.to_string(), value),
        Span::styled(" | ", sep),
        Span::styled("Active: ", label),
        Span::styled(active.to_string(), value),
        Span::styled(" | ", sep),
        Span::styled("Issues: ", label),
        Span::styled(st.issues.len().to_string(), value),
        Span::styled(" | ", sep),
        Span::styled("Done: ", label),
        Span::styled(st.done.to_string(), value),
        Span::styled(" ]", bracket),
    ];

    let next = next_task_label(st);
    let right_spans: Vec<Span> = vec![
        Span::styled("[ ", bracket),
        Span::styled("Next: ", label),
        Span::styled(next.clone(), value),
        Span::styled(" ]", bracket),
    ];

    let right_text_len = "[ Next:  ]".len() + next.len();
    let right_w = (right_text_len as u16).min(area.width);
    let left_w = area.width.saturating_sub(right_w);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(left_w), Constraint::Length(right_w)])
        .split(area);

    f.render_widget(Paragraph::new(Line::from(left_spans)), chunks[0]);
    f.render_widget(Paragraph::new(Line::from(right_spans)), chunks[1]);
}

fn render_progress_bar(f: &mut Frame, st: &BoardState, area: Rect) {
    let total = st.pending + st.done;
    let pct = st.done.saturating_mul(100).checked_div(total).unwrap_or(0);
    let label = format!(" {pct}%");
    let bar_width = (area.width as usize).saturating_sub(label.len() + 2);
    let filled = bar_width * pct / 100;
    let empty = bar_width.saturating_sub(filled);

    let line = Line::from(vec![
        Span::styled("[", Style::default().fg(ink(Ink::Muted))),
        Span::styled(
            "█".repeat(filled),
            Style::default()
                .fg(ink(Ink::Ok))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("░".repeat(empty), Style::default().fg(ink(Ink::Muted))),
        Span::styled("]", Style::default().fg(ink(Ink::Muted))),
        Span::styled(
            label,
            Style::default()
                .fg(ink(Ink::Text))
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_priority_legend(f: &mut Frame, st: &BoardState, area: Rect) {
    let label_area = Rect { height: 1, ..area };
    let legend_area = Rect {
        y: area.y + 1,
        height: area.height.saturating_sub(1),
        ..area
    };

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Priority:",
            Style::default()
                .fg(ink(Ink::Text))
                .add_modifier(Modifier::BOLD),
        ))),
        label_area,
    );

    let all: Vec<&Task> = st
        .issues
        .iter()
        .flat_map(|i| i.tasks.iter())
        .chain(st.standalone.iter())
        .collect();
    let total = all.len();
    if total == 0 {
        return;
    }

    let h = all
        .iter()
        .filter(|t| matches!(t.priority, Some(Priority::H)))
        .count();
    let m = all
        .iter()
        .filter(|t| matches!(t.priority, Some(Priority::M)))
        .count();
    let l = all
        .iter()
        .filter(|t| matches!(t.priority, Some(Priority::L)))
        .count();
    let n = total - h - m - l;

    const SWATCH: &str = "███";
    let mut spans = Vec::new();
    for (name, count, color) in [
        ("High", h, ink(Ink::Err)),
        ("Med", m, ink(Ink::Warn)),
        ("Low", l, ink(Ink::Ok)),
        ("None", n, ink(Ink::Muted)),
    ] {
        if count == 0 {
            continue;
        }
        let pct = count * 100 / total;
        spans.push(Span::styled(
            format!("{name} "),
            Style::default().fg(ink(Ink::Soft)),
        ));
        spans.push(Span::styled("[", Style::default().fg(ink(Ink::Muted))));
        spans.push(Span::styled(SWATCH, Style::default().fg(color)));
        spans.push(Span::styled("]", Style::default().fg(ink(Ink::Muted))));
        spans.push(Span::styled(
            format!(" {pct}%   "),
            Style::default().fg(ink(Ink::Soft)),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), legend_area);
}

pub(super) const MIN_SIZE: (u16, u16) = (60, 12);

fn render(f: &mut Frame, st: &BoardState, lines: &[Line]) {
    render_with(f, st, lines, None);
}

fn render_with(f: &mut Frame, st: &BoardState, lines: &[Line], pane: Option<&[Line<'static>]>) {
    if crate::infrastructure::tui::screen::too_small(f, MIN_SIZE.0, MIN_SIZE.1) {
        return;
    }
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    render_stats(f, st, chunks[0]);
    render_progress_bar(f, st, chunks[1]);
    render_priority_legend(f, st, chunks[3]);

    let (box_area, pane_area) = match (pane, preview_width(area.width)) {
        (Some(_), Some(w)) => {
            let split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(0), Constraint::Length(w + 2)])
                .split(chunks[5]);
            (split[0], Some(split[1]))
        }
        _ => (chunks[5], None),
    };

    let mut title = format!(" {} ", st.project);
    if !st.filter.is_empty() && !st.filtering {
        title.push_str(&format!("· / {} ", st.filter));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ink(Ink::Accent)))
        .title(Span::styled(
            title,
            Style::default()
                .fg(ink(Ink::Accent))
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(box_area);
    f.render_widget(block, box_area);

    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    f.render_widget(Paragraph::new(col_header_line()), inner_chunks[0]);
    if lines.is_empty() && !st.filter.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("  no tasks match “{}”", st.filter),
                Style::default().fg(ink(Ink::Muted)),
            ))),
            inner_chunks[1],
        );
    } else {
        f.render_widget(
            Paragraph::new(lines.to_vec()).scroll((st.scroll, 0)),
            inner_chunks[1],
        );
    }

    if let (Some(area), Some(body)) = (pane_area, pane) {
        render_preview(f, area, body);
    }

    f.render_widget(Paragraph::new(footer_line(st)), chunks[6]);
}

fn render_preview(f: &mut Frame, area: Rect, body: &[Line<'static>]) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ink(Ink::Muted)))
        .title(Span::styled(
            " preview ",
            Style::default()
                .fg(ink(Ink::Accent))
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if body.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " select a task to preview it",
                Style::default().fg(ink(Ink::Muted)),
            ))),
            inner,
        );
    } else {
        f.render_widget(Paragraph::new(body.to_vec()), inner);
    }
}

fn footer_line(st: &BoardState) -> Line<'static> {
    let key = Style::default()
        .fg(ink(Ink::Text))
        .add_modifier(Modifier::BOLD);
    let label = Style::default().fg(ink(Ink::Muted));
    if st.filtering {
        return Line::from(vec![
            Span::styled(
                " / ",
                Style::default()
                    .fg(ink(Ink::Accent))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(st.filter.clone(), key),
            Span::styled("▏", Style::default().fg(ink(Ink::Accent))),
            Span::styled("   Enter", key),
            Span::styled(" keep", label),
            Span::styled("  ·  ", label),
            Span::styled("Esc", key),
            Span::styled(" clear", label),
            Span::styled("  ·  ", label),
            Span::styled("+tag", key),
            Span::styled(" match a tag", label),
        ]);
    }
    let rows = visible_rows(st);
    let on_issue = matches!(rows.get(st.selected), Some(Row::Issue(_)));
    let mut pairs: Vec<(&str, &str)> = vec![("j/k", "move")];
    if on_issue {
        pairs.push(("h/l", "fold"));
    } else {
        pairs.push(("Enter", "open"));
    }
    pairs.push(("/", "filter"));
    if !st.filter.is_empty() {
        pairs.push(("Esc", "clear filter"));
    }
    pairs.push((
        "p",
        if st.preview {
            "hide preview"
        } else {
            "preview"
        },
    ));
    pairs.push(("?", "help"));
    pairs.push(("q", "quit"));
    let mut spans = vec![Span::raw(" ")];
    for (i, (k, l)) in pairs.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ·  ", label));
        }
        spans.push(Span::styled(k.to_string(), key));
        spans.push(Span::styled(format!(" {l}"), label));
    }
    Line::from(spans)
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/board/render.rs"]
mod tests;
