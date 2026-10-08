mod body;
mod header;
mod lines;
mod panels;
mod tree;

use std::rc::Rc;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

use crate::commands::info::types::EditState;
use body::{Body, body_pane};
use header::header_bar;
use panels::{add_step_box, comment_box, edit_box, footer, history_pane, side_panel};

pub(super) fn preview(st: &EditState, width: usize) -> Vec<ratatui::text::Line<'static>> {
    Body::build(st, false, width).0
}

pub(super) const MIN_SIZE: (u16, u16) = (60, 16);

pub(super) const PANEL_MIN_WIDTH: u16 = 96;
const PANEL_WIDTH: u16 = 42;

pub(super) fn render(f: &mut Frame, st: &mut EditState) {
    if crate::infrastructure::tui::screen::too_small(f, MIN_SIZE.0, MIN_SIZE.1) {
        return;
    }
    let show_panel = f.area().width >= PANEL_MIN_WIDTH;
    let history_height: u16 = if show_panel || st.detail.history.is_empty() {
        0
    } else {
        (st.detail.history.len() as u16 + 2).min(6)
    };
    let chunks = screen_chunks(f.area(), st, history_height);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(chunks[0]);
    let (main_area, panel_area) = if show_panel {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(PANEL_WIDTH), Constraint::Min(50)])
            .split(rows[1]);
        (cols[1], Some(cols[0]))
    } else {
        (rows[1], None)
    };
    let inner_width = main_area.width.saturating_sub(2) as usize;
    let (lines, sel_range) = Body::build(st, show_panel, inner_width);

    let buf = f.buffer_mut();
    header_bar(&st.detail, rows[0], buf);
    body_pane(st, lines, sel_range, main_area, buf);
    if let Some(panel) = panel_area {
        side_panel(st, panel, buf);
    }
    if history_height > 0 {
        history_pane(&st.detail, chunks[1], buf);
    }
    let input_area = chunks[if history_height > 0 { 2 } else { 1 }];
    if st.adding_step {
        add_step_box(st, input_area, buf);
    }
    if st.commenting {
        comment_box(st, input_area, buf);
    }
    if st.editing {
        edit_box(st, input_area, buf);
    }
    footer(st, chunks[chunks.len() - 1], buf);
}

fn screen_chunks(area: Rect, st: &EditState, history_height: u16) -> Rc<[Rect]> {
    let constraints = if st.editing || st.commenting || st.adding_step {
        if history_height > 0 {
            vec![
                Constraint::Min(1),
                Constraint::Length(history_height),
                Constraint::Length(3),
                Constraint::Length(1),
            ]
        } else {
            vec![
                Constraint::Min(1),
                Constraint::Length(3),
                Constraint::Length(1),
            ]
        }
    } else if history_height > 0 {
        vec![
            Constraint::Min(1),
            Constraint::Length(history_height),
            Constraint::Length(1),
        ]
    } else {
        vec![Constraint::Min(1), Constraint::Length(1)]
    };
    Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area)
}

#[cfg(test)]
#[path = "../../../../tests/unit/commands/info/render.rs"]
mod tests;
