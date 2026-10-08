mod activity;
mod checklist;
mod fields;
mod notes;
mod refs;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

use crate::infrastructure::tui::theme::{Ink, ink};

use crate::commands::info::handler::focusables;
use crate::commands::info::types::{Detail, EditState, Focusable};

pub(super) struct Body<'a> {
    d: &'a Detail,
    st: &'a EditState,
    items: Vec<Focusable>,
    sel: Option<Focusable>,
    show_panel: bool,
    lines: Vec<Line<'static>>,
    sel_range: Option<(usize, usize)>,
}

impl<'a> Body<'a> {
    pub(super) fn build(
        st: &'a EditState,
        show_panel: bool,
    ) -> (Vec<Line<'static>>, Option<(usize, usize)>) {
        let d = &st.detail;
        let items = focusables(d, st.show_notes);
        let sel = if st.editing {
            None
        } else {
            items.get(st.selected).cloned()
        };
        let mut body = Body {
            d,
            st,
            items,
            sel,
            show_panel,
            lines: vec![],
            sel_range: None,
        };
        body.anchor();
        body.edit_rows();
        body.status_row();
        body.time_row();
        body.urgency_row();
        body.date_rows();
        body.select_hint();
        body.typed_notes();
        body.blockers();
        body.cited();
        body.links();
        body.files();
        body.anchors();
        body.checklist();
        body.verification();
        body.ai_activity();
        body.related_tasks();
        body.comments();
        (body.lines, body.sel_range)
    }
}

pub(super) fn body_pane(
    st: &mut EditState,
    lines: Vec<Line<'static>>,
    sel_range: Option<(usize, usize)>,
    area: Rect,
    buf: &mut Buffer,
) {
    let t = &st.detail.task;
    let title = format!(
        " Task {}{} ",
        t.id.map(|i| i.to_string()).unwrap_or_else(|| "-".into()),
        if t.is_active() { "  ● ACTIVE" } else { "" }
    );

    let inner_w = area.width.saturating_sub(2).max(1);
    let viewport = area.height.saturating_sub(2) as usize;
    let selection_moved = st.last_selected != Some(st.selected);
    st.last_selected = Some(st.selected);
    if viewport > 0 {
        if selection_moved && let Some((first, last)) = sel_range {
            let top: usize = lines[..first]
                .iter()
                .map(|l| wrapped_rows(l, inner_w))
                .sum();
            let bottom: usize = top
                + lines[first..=last]
                    .iter()
                    .map(|l| wrapped_rows(l, inner_w))
                    .sum::<usize>();
            let mut scroll = st.scroll as usize;
            if bottom > scroll + viewport {
                scroll = bottom - viewport;
            }
            if top < scroll {
                scroll = top;
            }
            st.scroll = scroll.min(u16::MAX as usize) as u16;
        }
        let total: usize = lines.iter().map(|l| wrapped_rows(l, inner_w)).sum();
        st.scroll = st
            .scroll
            .min(total.saturating_sub(viewport).min(u16::MAX as usize) as u16);
    }

    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(ink(Ink::Accent))),
        )
        .wrap(Wrap { trim: false })
        .scroll((st.scroll, 0))
        .render(area, buf);
}

fn wrapped_rows(line: &Line, width: u16) -> usize {
    let width = width.max(1) as usize;
    if line.width() <= width {
        return 1;
    }
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    let mut rows = 1usize;
    let mut used = 0usize;
    for word in text.split(' ') {
        let w = Span::raw(word).width();
        let sep = usize::from(used > 0);
        if used + sep + w <= width {
            used += sep + w;
        } else if w > width {
            if used > 0 {
                rows += 1;
            }
            let mut rem = w;
            while rem > width {
                rows += 1;
                rem -= width;
            }
            used = rem;
        } else {
            rows += 1;
            used = w;
        }
    }
    rows
}
