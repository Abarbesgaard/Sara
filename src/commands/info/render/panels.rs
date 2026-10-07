use chrono::Local;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

use crate::infrastructure::tui::theme::{Ink, ink};

use super::tree::task_tree_lines;
use crate::commands::info::handler::{comment_target, focusables};
use crate::commands::info::types::{Detail, EDIT_FIELDS, EditField, EditState};

pub(super) fn side_panel(st: &EditState, area: Rect, buf: &mut Buffer) {
    let d = &st.detail;
    let tree_lines = task_tree_lines(d, st);
    let top_h: u16 = ((tree_lines.len() + 2) as u16).clamp(7, 24);

    let panel_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(top_h), Constraint::Min(4)])
        .split(area);

    let tree_title = if st.tree_expanded {
        " Task tree — expanded  (d to collapse) "
    } else {
        " Task tree  (d to expand) "
    };
    Paragraph::new(tree_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(tree_title)
                .border_style(Style::default().fg(ink(Ink::Special))),
        )
        .render(panel_chunks[0], buf);

    git_panel(d, panel_chunks[1], buf);
}

fn git_panel(d: &Detail, area: Rect, buf: &mut Buffer) {
    Paragraph::new(git_panel_lines(d))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Git ")
                .border_style(Style::default().fg(ink(Ink::Muted))),
        )
        .wrap(Wrap { trim: false })
        .render(area, buf);
}

pub(super) fn history_pane(d: &Detail, area: Rect, buf: &mut Buffer) {
    Paragraph::new(history_lines(&d.history))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" History ")
                .border_style(Style::default().fg(ink(Ink::Muted))),
        )
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
    let items = focusables(d, st.show_notes);
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
    let footer = if st.adding_step {
        " type a step  •  Enter/Ctrl+S save  •  Esc cancel ".to_string()
    } else if st.commenting {
        " type a comment  •  Enter/Ctrl+S save  •  Esc cancel ".to_string()
    } else if st.editing {
        " type to edit  •  Enter/Ctrl+S confirm  •  Esc cancel ".to_string()
    } else {
        " ↑/↓ move • Enter edit/open • c comment • a step • d tree • n notes • u urgency • v expand • ? help • q close "
            .to_string()
    };
    Paragraph::new(footer)
        .style(Style::default().fg(ink(Ink::Soft)))
        .render(area, buf);
}

pub(in crate::commands::info) fn history_lines(
    history: &[crate::infrastructure::db::HistoryEntry],
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
        let mut spans = vec![
            Span::styled(format!("  {date}  "), Style::default().fg(ink(Ink::Muted))),
            Span::styled(
                format!("{:<11} ", label),
                Style::default().fg(ink(Ink::Accent)),
            ),
        ];
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
    let mut lines: Vec<Line<'static>> = vec![];

    let Some(rec) = &d.branch else {
        lines.push(Line::from(Span::styled(
            "  No branch tied.",
            Style::default().fg(ink(Ink::Muted)),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Run: sara <id> addbranch",
            Style::default().fg(ink(Ink::Soft)),
        )));
        return lines;
    };

    lines.push(Line::from(vec![
        Span::styled("  Branch  ", Style::default().fg(ink(Ink::Muted))),
        Span::styled(
            rec.branch.clone(),
            Style::default()
                .fg(ink(Ink::Accent))
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    lines
}
