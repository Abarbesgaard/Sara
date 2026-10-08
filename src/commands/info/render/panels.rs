use chrono::Local;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

use crate::infrastructure::tui::theme::{Ink, ink};

use super::lines::wrap_hanging;
use super::tree::task_tree_lines;
use crate::commands::info::handler::{comment_target, focusables};
use crate::commands::info::types::{Detail, EDIT_FIELDS, EditField, EditState, Focusable};

pub(super) fn side_panel(st: &EditState, area: Rect, buf: &mut Buffer) {
    let d = &st.detail;
    let tree_lines = task_tree_lines(d, st);
    let has_history = !d.history.is_empty();
    let tree_h: u16 = ((tree_lines.len() + 2) as u16).clamp(5, 24);
    let mut constraints = vec![if has_history || d.branch.is_some() {
        Constraint::Length(tree_h)
    } else {
        Constraint::Min(tree_h)
    }];
    if d.branch.is_some() {
        constraints.push(if has_history {
            Constraint::Length(3)
        } else {
            Constraint::Min(3)
        });
    }
    if has_history {
        constraints.push(Constraint::Min(3));
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let tree_title = if st.tree_expanded {
        " TASK TREE · EXPANDED "
    } else {
        " TASK TREE "
    };
    Paragraph::new(tree_lines)
        .block(panel_block(tree_title, ink(Ink::Special)))
        .render(chunks[0], buf);
    let mut next = 1;
    if d.branch.is_some() {
        Paragraph::new(git_panel_lines(d))
            .block(panel_block(" GIT ", ink(Ink::Muted)))
            .render(chunks[next], buf);
        next += 1;
    }
    if has_history {
        let width = chunks[next].width.saturating_sub(2) as usize;
        let (lines, _) = wrap_hanging(history_lines(&d.history, true), width, None);
        Paragraph::new(lines)
            .block(panel_block(" HISTORY ", ink(Ink::Muted)))
            .render(chunks[next], buf);
    }
}

fn panel_block(title: &str, color: ratatui::style::Color) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(color))
}

pub(super) fn history_pane(d: &Detail, area: Rect, buf: &mut Buffer) {
    Paragraph::new(history_lines(&d.history, false))
        .block(panel_block(" HISTORY ", ink(Ink::Muted)))
        .wrap(Wrap { trim: false })
        .render(area, buf);
}

pub(super) fn add_step_box(st: &EditState, area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Add step  (Enter save · Esc cancel) ".to_string())
        .border_style(Style::default().fg(ink(Ink::Ok)));
    editor_box(st, block, area, buf);
}

pub(super) fn comment_box(st: &EditState, area: Rect, buf: &mut Buffer) {
    let d = &st.detail;
    let items = focusables(d, st.show_notes, &st.open);
    let focus = items.get(st.selected).cloned();
    let (tk, tid) = comment_target(d, &focus);
    let target = match (tk, tid) {
        (Some(k), Some(i)) => format!("{k}:{i}"),
        _ => "task".to_string(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Comment on {target}  (Enter save · Esc cancel) "))
        .border_style(Style::default().fg(ink(Ink::Warn)));
    editor_box(st, block, area, buf);
}

pub(super) fn edit_box(st: &EditState, area: Rect, buf: &mut Buffer) {
    let field = EDIT_FIELDS
        .get(st.selected)
        .copied()
        .unwrap_or(EditField::Description);
    let (title, border) = if st.due_error {
        (
            format!(" Editing {} — invalid date ", field.label()),
            ink(Ink::Err),
        )
    } else if let Some(ref err) = st.dep_error {
        (
            format!(" Editing {} — {} ", field.label(), err),
            ink(Ink::Err),
        )
    } else if field == EditField::DependsOn {
        (
            format!(
                " Editing {}  (task IDs, space/comma separated · Enter confirm · Esc cancel) ",
                field.label()
            ),
            ink(Ink::Warn),
        )
    } else {
        (
            format!(" Editing {}  (Enter confirm · Esc cancel) ", field.label()),
            ink(Ink::Warn),
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(border));
    editor_box(st, block, area, buf);
}

fn editor_box(st: &EditState, block: Block, area: Rect, buf: &mut Buffer) {
    let inner = block.inner(area);
    block.render(area, buf);
    (&st.editor).render(inner, buf);
}

pub(super) fn footer(st: &EditState, area: Rect, buf: &mut Buffer) {
    let keys: Vec<(&str, &str)> = if st.adding_step {
        vec![("⏎", "save step"), ("Esc", "cancel")]
    } else if st.commenting {
        vec![("⏎", "save comment"), ("Esc", "cancel")]
    } else if st.editing {
        vec![("⏎", "confirm"), ("Esc", "cancel")]
    } else {
        let items = focusables(&st.detail, st.show_notes, &st.open);
        let mut keys = focus_keys(items.get(st.selected));
        keys.extend([
            ("j/k", "move"),
            ("a", "add step"),
            ("?", "help"),
            ("q", "close"),
        ]);
        keys
    };
    let mut spans = vec![Span::raw(" ")];
    for (i, (k, label)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("   ", Style::default()));
        }
        spans.push(Span::styled(
            k.to_string(),
            Style::default()
                .fg(ink(Ink::Accent))
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(ink(Ink::Soft)),
        ));
    }
    Paragraph::new(Line::from(spans)).render(area, buf);
}

fn focus_keys(focus: Option<&Focusable>) -> Vec<(&'static str, &'static str)> {
    match focus {
        Some(Focusable::Field(EditField::Priority)) => vec![("←/→", "priority")],
        Some(Focusable::Field(_)) => vec![("e", "edit")],
        Some(Focusable::Section(_)) => vec![("⏎", "fold")],
        Some(Focusable::Checklist(_)) => {
            vec![("␣", "toggle"), ("K/J", "reorder"), ("c", "comment")]
        }
        Some(Focusable::Memory(_)) => vec![("⏎", "open in dream")],
        Some(Focusable::Link(_)) => vec![("⏎", "open"), ("c", "comment")],
        Some(Focusable::File(_)) => vec![("⏎", "open")],
        Some(Focusable::Anchor(_)) => vec![("⏎", "open"), ("r", "reconsider"), ("x", "resolve")],
        Some(Focusable::Note(_)) => vec![("c", "reply"), ("r", "reconsider"), ("x", "resolve")],
        Some(Focusable::Comment(_)) => vec![("⏎", "reply"), ("x", "resolve")],
        None => vec![],
    }
}

pub(in crate::commands::info) fn history_lines(
    history: &[crate::infrastructure::db::HistoryEntry],
    compact: bool,
) -> Vec<Line<'static>> {
    let mut lines = vec![];
    for h in history.iter().rev() {
        let date = h
            .changed_at
            .with_timezone(&Local)
            .format("%m-%d %H:%M")
            .to_string();
        let label = if h.field == "annotation" {
            "comment"
        } else {
            &h.field
        };
        let head = vec![
            Span::styled(format!(" {date}  "), Style::default().fg(ink(Ink::Muted))),
            Span::styled(
                if compact {
                    label.to_string()
                } else {
                    format!("{label:<11} ")
                },
                Style::default().fg(ink(Ink::Accent)),
            ),
        ];
        let mut spans = if compact {
            lines.push(Line::from(head));
            vec![Span::raw("   ")]
        } else {
            let mut head = head;
            head.insert(0, Span::raw(" "));
            head
        };
        let additive = matches!(
            h.field.as_str(),
            "annotation" | "link" | "dependency" | "checklist" | "file"
        ) && h.old_value.is_none() != h.new_value.is_none();
        if h.field == "created" {
            spans.push(Span::raw(h.new_value.clone().unwrap_or_default()));
        } else if additive {
            if let Some(text) = &h.new_value {
                spans.push(Span::styled("+ ", Style::default().fg(ink(Ink::Ok))));
                spans.push(Span::raw(text.clone()));
            } else if let Some(text) = &h.old_value {
                spans.push(Span::styled("− ", Style::default().fg(ink(Ink::Err))));
                spans.push(Span::raw(text.clone()));
            }
        } else {
            spans.push(Span::styled(
                h.old_value.clone().unwrap_or_else(|| "—".into()),
                Style::default().fg(ink(Ink::Soft)),
            ));
            spans.push(Span::styled(" → ", Style::default().fg(ink(Ink::Muted))));
            spans.push(Span::raw(h.new_value.clone().unwrap_or_else(|| "—".into())));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn git_panel_lines(d: &Detail) -> Vec<Line<'static>> {
    let Some(rec) = &d.branch else {
        return vec![];
    };
    vec![Line::from(vec![
        Span::styled(" ⎇ ", Style::default().fg(ink(Ink::Muted))),
        Span::styled(
            rec.branch.clone(),
            Style::default()
                .fg(ink(Ink::Accent))
                .add_modifier(Modifier::BOLD),
        ),
    ])]
}
