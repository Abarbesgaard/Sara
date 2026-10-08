mod activity;
mod checklist;
mod fields;
mod notes;
mod refs;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};

use crate::infrastructure::tui::theme::{Ink, ink};

use super::lines::{label_line, wrap_hanging};
use crate::commands::info::handler::{focusables, section_len};
use crate::commands::info::types::{Detail, EditState, Focusable, SectionId};

pub(super) struct Body<'a> {
    d: &'a Detail,
    st: &'a EditState,
    items: Vec<Focusable>,
    sel: Option<Focusable>,
    show_panel: bool,
    width: usize,
    lines: Vec<Line<'static>>,
    sel_range: Option<(usize, usize)>,
}

impl<'a> Body<'a> {
    pub(super) fn build(
        st: &'a EditState,
        show_panel: bool,
        width: usize,
    ) -> (Vec<Line<'static>>, Option<(usize, usize)>) {
        let d = &st.detail;
        let items = focusables(d, st.show_notes, &st.open);
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
            width,
            lines: vec![],
            sel_range: None,
        };
        body.hero();
        body.anchor();
        body.details();
        body.typed_notes();
        body.blockers();
        body.ledger();
        body.links();
        body.files();
        body.anchors();
        body.checklist();
        body.verification();
        body.ai_activity();
        body.related_tasks();
        body.comments();
        wrap_hanging(body.lines, width, body.sel_range)
    }
}

impl Body<'_> {
    fn label(&mut self, title: &str) {
        self.lines.push(Line::from(""));
        self.lines
            .push(label_line("▌", title, None, self.width, false));
    }

    fn fold(&mut self, id: SectionId, title: &str) -> bool {
        let open = self.st.open.contains(&id);
        let selected = self.sel == Some(Focusable::Section(id));
        let glyph = if open { "▾" } else { "▸" };
        self.lines.push(Line::from(""));
        if selected {
            self.sel_range = Some((self.lines.len(), self.lines.len()));
        }
        self.lines.push(label_line(
            glyph,
            title,
            Some(section_len(self.d, id)),
            self.width,
            selected,
        ));
        open
    }
}

pub(super) fn body_pane(
    st: &mut EditState,
    lines: Vec<Line<'static>>,
    sel_range: Option<(usize, usize)>,
    area: Rect,
    buf: &mut Buffer,
) {
    let project = &st.detail.task.project;
    let title = format!(" {} ", if project.is_empty() { "task" } else { project });
    let viewport = area.height.saturating_sub(2) as usize;
    let selection_moved = st.last_selected != Some(st.selected);
    st.last_selected = Some(st.selected);
    if viewport > 0 {
        if selection_moved && let Some((top, last)) = sel_range {
            let bottom = last + 1;
            let mut scroll = st.scroll as usize;
            if bottom > scroll + viewport {
                scroll = bottom - viewport;
            }
            if top < scroll {
                scroll = top;
            }
            st.scroll = scroll.min(u16::MAX as usize) as u16;
        }
        st.scroll = st
            .scroll
            .min(lines.len().saturating_sub(viewport).min(u16::MAX as usize) as u16);
    }

    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .title(title)
                .border_style(Style::default().fg(ink(Ink::Accent))),
        )
        .scroll((st.scroll, 0))
        .render(area, buf);
}
