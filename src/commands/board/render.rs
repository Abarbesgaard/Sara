use anyhow::Result;
use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
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

use super::{BoardAction, BoardState, IssueNode};

/// Fixed rows consumed outside the scrollable task list: 6 header lines
/// (stats, progress, blank, priority label, priority legend, blank) + 2
/// box borders + 1 in-box column header + 1 footer line.
const FIXED_OVERHEAD: u16 = 10;

/// One selectable row in the flattened, expansion-aware tree.
#[derive(Clone, Copy)]
pub(super) enum Row {
    Issue(usize),
    Task(usize, usize),
    Standalone(usize),
}

/// Flatten the tree into the rows currently on screen.
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
        let size = terminal.size()?;
        let viewport = size.height.saturating_sub(FIXED_OVERHEAD);
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
        CONFIRM,
        QUIT,
        HELP,
    ]
}

fn build_lines(st: &BoardState, rows: &[Row]) -> (Vec<Line<'static>>, Vec<u16>) {
    let mut lines: Vec<Line> = Vec::with_capacity(rows.len());
    let mut row_line: Vec<u16> = vec![0; rows.len()];

    for (i, row) in rows.iter().enumerate() {
        row_line[i] = lines.len() as u16;
        let is_sel = i == st.selected;
        match *row {
            Row::Issue(gi) => lines.push(issue_header(&st.issues[gi], is_sel)),
            Row::Task(gi, ti) => {
                let issue = &st.issues[gi];
                let task = &issue.tasks[ti];
                let connector = if ti + 1 == issue.tasks.len() {
                    "└─"
                } else {
                    "├─"
                };
                lines.push(task_line_for(
                    task,
                    is_sel,
                    Some(connector),
                    badge_for(st, task),
                ));
            }
            Row::Standalone(si) => {
                let task = &st.standalone[si];
                lines.push(task_line_for(task, is_sel, None, badge_for(st, task)));
            }
        }
    }
    (lines, row_line)
}

fn badge_for(st: &BoardState, task: &Task) -> Span<'static> {
    let uuid = task.uuid.to_string();
    let flags = st.badges.get(&uuid).copied().unwrap_or_default();
    let synced = st.imported.contains(&uuid);
    board_badge_span(flags, synced)
}

/// Always returns a fixed-`BADGE_W`-wide span (blank when there's nothing to
/// show) so the description column starts at the same x on every row.
fn board_badge_span(flags: LinkFlags, synced: bool) -> Span<'static> {
    let (label, color) = if flags.pr {
        ("PR", Color::Magenta)
    } else if flags.issue && synced {
        ("ISS", Color::Green)
    } else if flags.any {
        ("↗", Color::Cyan)
    } else {
        ("", Color::Reset)
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

// ── Column layout ────────────────────────────────────────────────────────────
//
// Fixed-width columns shared by every row (issue header, nested task,
// standalone task) and the header line, so the eye can scan straight down
// each column instead of hunting for it on every row:
//
//  col 0          : selection marker (1)   ▶ or space
//  col 1..1+TREE_W: tree connector          ▾/▸ / ├─ / └─ / blank
//  ID_W            right-aligned task id
//  2-space separator
//  PRI_W            colored priority chip (background fill, not just text)
//  1-space separator
//  AGE_W            age since creation, left-aligned
//  2-space separator
//  BADGE_W          PR/ISS/link badge, always reserved so it never shifts
//                   the description column
//  DESCRIPTION      remaining width

const TREE_W: usize = 3;
const ID_W: usize = 3;
const PRI_W: usize = 3;
const AGE_W: usize = 7;
const BADGE_W: usize = 5;

/// Column header line, built from the exact same widths as the data rows so
/// the labels always land directly above their column, however the widths
/// above are tuned.
fn col_header_line() -> Line<'static> {
    let mut s = String::new();
    s.push(' '); // selection marker column
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
            .fg(Color::White)
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

/// A `PRI_W`-wide priority chip with a solid background fill — a color you
/// can scan for down the column, rather than dim foreground text that's easy
/// to miss at a glance. Selection always wins (flat white-on-blue) so the
/// chip doesn't fight the row highlight.
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
                .fg(Color::White)
                .bg(row_bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    match pri {
        Some(Priority::H) => Span::styled(
            text,
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        Some(Priority::M) => Span::styled(
            text,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Some(Priority::L) => Span::styled(
            text,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        None => Span::styled(text, Style::default().fg(Color::DarkGray).bg(row_bg)),
    }
}

/// Issue-group header row, tinted with a subtle full-row background so groups
/// read as section dividers at a glance instead of blending into their child
/// rows. Fills PRI with the highest priority across visible tasks, AGE with
/// the oldest entry timestamp, so every column stays populated.
fn issue_header(issue: &IssueNode, is_sel: bool) -> Line<'static> {
    // Subtle indigo tint distinguishes a group header from its plain-background
    // child task rows without competing with the blue selection highlight.
    let bg = if is_sel {
        Color::Blue
    } else {
        Color::Rgb(28, 30, 44)
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
        Color::White
    } else if complete {
        Color::Green
    } else {
        Color::Cyan
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
    let count_str = format!("  [{done}/{total} done]");

    let meta = if is_sel {
        Style::default().fg(Color::White).bg(bg)
    } else {
        Style::default().fg(Color::DarkGray).bg(bg)
    };

    Line::from(vec![
        Span::styled(if is_sel { "▶" } else { " " }, meta),
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
        Span::styled(count_str, meta),
    ])
}

/// `connector`: `Some("├─")` / `Some("└─")` for nested tasks, `None` for
/// standalone. Nested tasks get one extra leading space beyond the tree
/// connector so they visibly indent past their issue header instead of
/// lining up flush with it.
fn task_line_for(
    task: &Task,
    is_sel: bool,
    connector: Option<&'static str>,
    badge_span: Span<'static>,
) -> Line<'static> {
    let bg = if is_sel { Color::Blue } else { Color::Reset };
    let sel_ch = if is_sel { "▶" } else { " " };
    let tree = match connector {
        Some(c) => format!(" {c}"),         // nested: extra indent + connector (3 wide)
        None => " ".repeat(TREE_W),          // standalone: blank (3 wide)
    };
    let id_str = task
        .id
        .map(|i| format!("{i:>w$}", w = ID_W))
        .unwrap_or_else(|| format!("{:>w$}", "-", w = ID_W));
    let age = age_str(task.entry);

    if task.status == Status::Completed {
        let base = Style::default()
            .fg(if is_sel { Color::White } else { Color::DarkGray })
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
        let s = Style::default().fg(Color::White).bg(bg);
        (s, s, s, s.add_modifier(Modifier::BOLD))
    } else {
        (
            Style::default().fg(Color::Gray).bg(bg),
            Style::default().fg(Color::Cyan).bg(bg),
            Style::default().fg(Color::DarkGray).bg(bg),
            Style::default().bg(bg),
        )
    };
    let spans = vec![
        Span::styled(format!("{sel_ch}{tree}"), meta_s),
        Span::styled(format!("{id_str}  "), id_s),
        priority_chip(task.priority.as_ref(), is_sel, bg),
        Span::styled(" ", meta_s),
        Span::styled(format!("{age:<w$}  ", w = AGE_W), age_s),
        badge_span,
        Span::styled(task.description.clone(), desc_s),
    ];
    Line::from(spans)
}

// ── Header widgets ────────────────────────────────────────────────────────────

fn active_count(st: &BoardState) -> usize {
    st.issues
        .iter()
        .flat_map(|i| &i.tasks)
        .chain(st.standalone.iter())
        .filter(|t| t.is_active())
        .count()
}

fn next_task_label(st: &BoardState) -> String {
    let next = st
        .issues
        .iter()
        .flat_map(|i| &i.tasks)
        .chain(st.standalone.iter())
        .filter(|t| t.status == Status::Pending)
        .max_by(|a, b| {
            a.urgency
                .partial_cmp(&b.urgency)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    match next {
        Some(t) => {
            let id = t.id.map(|i| format!("#{i} ")).unwrap_or_default();
            format!("{}{}", id, truncate(&t.description, 24))
        }
        None => "—".to_string(),
    }
}

/// Stats row: cyan brackets, dimmed labels, bold-white values.
fn render_stats(f: &mut Frame, st: &BoardState, area: Rect) {
    let total = st.pending + st.done;
    let active = active_count(st);

    let bracket = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let label = Style::default().fg(Color::DarkGray);
    let value = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let sep = Style::default().fg(Color::DarkGray);

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

/// Full-width progress bar: filled/empty blocks with a trailing percentage.
fn render_progress_bar(f: &mut Frame, st: &BoardState, area: Rect) {
    let total = st.pending + st.done;
    let pct = if total == 0 { 0 } else { st.done * 100 / total };
    let label = format!(" {pct}%");
    let bar_width = (area.width as usize).saturating_sub(label.len() + 2);
    let filled = bar_width * pct / 100;
    let empty = bar_width.saturating_sub(filled);

    let line = Line::from(vec![
        Span::styled("[", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "█".repeat(filled),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("░".repeat(empty), Style::default().fg(Color::DarkGray)),
        Span::styled("]", Style::default().fg(Color::DarkGray)),
        Span::styled(
            label,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

/// Priority legend, mirroring htop's "Languages:" line: a label line followed
/// by a row of `NAME [swatch] NN%` entries. Each swatch is a small fixed-width
/// color block (a legend key, not a proportional bar) — matching htop's style.
fn render_priority_legend(f: &mut Frame, st: &BoardState, area: Rect) {
    let label_area = Rect {
        height: 1,
        ..area
    };
    let legend_area = Rect {
        y: area.y + 1,
        height: area.height.saturating_sub(1),
        ..area
    };

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Priority:",
            Style::default()
                .fg(Color::White)
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
        ("High", h, Color::Red),
        ("Med", m, Color::Yellow),
        ("Low", l, Color::Green),
        ("None", n, Color::DarkGray),
    ] {
        if count == 0 {
            continue;
        }
        let pct = count * 100 / total;
        spans.push(Span::styled(
            format!("{name} "),
            Style::default().fg(Color::Gray),
        ));
        spans.push(Span::styled("[", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(SWATCH, Style::default().fg(color)));
        spans.push(Span::styled("]", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(
            format!(" {pct}%   "),
            Style::default().fg(Color::Gray),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), legend_area);
}

fn render(f: &mut Frame, st: &BoardState, lines: &[Line]) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // stats
            Constraint::Length(1), // progress bar
            Constraint::Length(1), // blank
            Constraint::Length(2), // priority legend (label + swatch row)
            Constraint::Length(1), // blank
            Constraint::Min(3),    // bordered task list box
            Constraint::Length(1), // navigation footer
        ])
        .split(area);

    render_stats(f, st, chunks[0]);
    render_progress_bar(f, st, chunks[1]);
    render_priority_legend(f, st, chunks[3]);

    let box_area = chunks[5];
    let title = format!(" {} ", st.project);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(box_area);
    f.render_widget(block, box_area);

    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    f.render_widget(Paragraph::new(col_header_line()), inner_chunks[0]);
    f.render_widget(
        Paragraph::new(lines.to_vec()).scroll((st.scroll, 0)),
        inner_chunks[1],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " j/k",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Select", Style::default().fg(Color::DarkGray)),
            Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "h/l",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Collapse/Expand", Style::default().fg(Color::DarkGray)),
            Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Open", Style::default().fg(Color::DarkGray)),
            Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "?",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Help", Style::default().fg(Color::DarkGray)),
            Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "q",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Quit", Style::default().fg(Color::DarkGray)),
        ])),
        chunks[6],
    );
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
        let done = tasks
            .iter()
            .filter(|t| t.status == Status::Completed)
            .count();
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

    /// Character (not byte) column of the first occurrence of `needle` in `line`.
    fn char_col_of(line: &str, needle: &str) -> usize {
        let chars: Vec<char> = line.chars().collect();
        let needle_chars: Vec<char> = needle.chars().collect();
        chars
            .windows(needle_chars.len())
            .position(|w| w == needle_chars.as_slice())
            .unwrap()
    }

    /// Character column of the first occurrence of any char in `candidates`.
    fn char_col_of_any(line: &str, candidates: &[char]) -> usize {
        line.chars()
            .position(|c| candidates.contains(&c))
            .unwrap()
    }

    /// A generously sized terminal so the bordered box + header/footer chrome
    /// all have room to render (the real board needs a real terminal size too).
    fn draw(st: &BoardState) -> String {
        let rows = visible_rows(st);
        let (lines, _) = build_lines(st, &rows);
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
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
        assert!(!out.lines().any(|l| l.contains('└') && l.contains("child")));
        assert!(!out.lines().any(|l| l.contains('├') && l.contains("child")));
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
        assert!(!draw(&st).contains("ISS"));
        st.imported.insert(uuid);
        assert!(draw(&st).contains("ISS"));
    }

    #[test]
    fn no_badge_for_task_without_links() {
        let t = Task::new("plain".into(), "tk".into());
        let st = board_state(vec![issue_node(5, vec![t], true)], vec![]);
        let out = draw(&st);
        assert!(!out.contains("PR "));
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
    fn nested_tasks_use_correct_tree_connectors() {
        let a = Task::new("alpha".into(), "tk".into());
        let b = Task::new("beta".into(), "tk".into());
        let st = board_state(vec![issue_node(5, vec![a, b], true)], vec![]);
        let out = draw(&st);
        assert!(out.lines().any(|l| l.contains("├─") && l.contains("alpha")));
        assert!(out.lines().any(|l| l.contains("└─") && l.contains("beta")));
    }

    #[test]
    fn nested_tasks_indent_further_than_their_issue_header() {
        let a = Task::new("alpha".into(), "tk".into());
        let st = board_state(vec![issue_node(5, vec![a], true)], vec![]);
        let out = draw(&st);
        let header_line = out
            .lines()
            .find(|l| l.starts_with('│') && l.contains("#5"))
            .unwrap();
        let task_line = out
            .lines()
            .find(|l| l.starts_with('│') && l.contains("alpha"))
            .unwrap();
        // Compare the column of the tree-connector glyph itself, not raw
        // leading spaces — the selection marker on the header row otherwise
        // throws off a naive whitespace count.
        let header_tree_col = char_col_of_any(header_line, &['▸', '▾']);
        let task_tree_col = char_col_of_any(task_line, &['├', '└']);
        assert_eq!(
            task_tree_col,
            header_tree_col + 1,
            "nested task's tree connector should sit one column past its issue header's"
        );
    }

    #[test]
    fn stats_row_shows_issue_count() {
        let t = Task::new("x".into(), "tk".into());
        let st = board_state(vec![issue_node(5, vec![t], false)], vec![]);
        assert!(draw(&st).contains("Issues:"));
    }

    #[test]
    fn task_box_has_a_border_and_project_title() {
        let t = Task::new("x".into(), "tk".into());
        let st = board_state(vec![issue_node(5, vec![t], false)], vec![]);
        let out = draw(&st);
        assert!(out.contains('╭'));
        assert!(out.contains('╰'));
        assert!(out.contains("tk"));
    }

    #[test]
    fn priority_legend_shows_swatches_not_proportional_bars() {
        let mut h = Task::new("h task".into(), "tk".into());
        h.priority = Some(Priority::H);
        let st = board_state(vec![], vec![h]);
        let out = draw(&st);
        assert!(out.contains("High"));
        assert!(out.contains("100%"));
    }

    #[test]
    fn description_columns_align_regardless_of_badges() {
        // Two standalone tasks, one with a PR badge and one without — both
        // descriptions must start at the same column since the badge slot
        // is always reserved.
        let a = Task::new("has badge".into(), "tk".into());
        let b = Task::new("no badge".into(), "tk".into());
        let uuid_a = a.uuid.to_string();
        let mut st = board_state(vec![], vec![a, b]);
        st.badges.insert(
            uuid_a,
            LinkFlags {
                any: true,
                pr: true,
                issue: false,
            },
        );
        let out = draw(&st);
        // Restrict to rows inside the bordered box — the stats row's "[ Next: ... ]"
        // label can otherwise coincidentally contain the same description text.
        let line_a = out
            .lines()
            .find(|l| l.starts_with('│') && l.contains("has badge"))
            .unwrap();
        let line_b = out
            .lines()
            .find(|l| l.starts_with('│') && l.contains("no badge"))
            .unwrap();
        let col_a = char_col_of(line_a, "has badge");
        let col_b = char_col_of(line_b, "no badge");
        assert_eq!(col_a, col_b, "description column must stay aligned");
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
