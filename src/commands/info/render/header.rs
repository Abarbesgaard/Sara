use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::commands::info::handler::guide_is_stale;
use crate::commands::info::types::Detail;
use crate::commands::shared::short_id;
use crate::infrastructure::model::Status;
use crate::infrastructure::tui::theme::{Ink, ink};

pub(super) fn header_bar(d: &Detail, area: Rect, buf: &mut Buffer) {
    let bg = ink(Ink::Header);
    let sep = || Span::raw("  ");
    let mut left = vec![Span::raw(" "), status_badge(d)];
    left.push(sep());
    left.push(freshness_badge(d));
    left.push(sep());
    left.push(branch_badge(d));
    left.push(sep());
    left.extend(feedback_badge(d));
    let ids = id_badge(d);
    let used: usize = left.iter().map(|s| s.width()).sum::<usize>() + ids.width();
    let pad = (area.width as usize).saturating_sub(used + 1);
    left.push(Span::raw(" ".repeat(pad.max(2))));
    left.push(ids);
    left.push(Span::raw(" "));
    Paragraph::new(Line::from(left))
        .style(Style::default().bg(bg))
        .render(area, buf);
}

fn status_badge(d: &Detail) -> Span<'static> {
    let t = &d.task;
    let (text, color) = if t.is_active() {
        ("[● ACTIVE]".to_string(), ink(Ink::Ok))
    } else {
        let color = match t.status {
            Status::Completed => ink(Ink::Ok),
            Status::Deleted => ink(Ink::Err),
            Status::Pending => ink(Ink::Text),
        };
        (
            format!("[○ {}]", t.status.to_string().to_uppercase()),
            color,
        )
    };
    Span::styled(
        text,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn freshness_badge(d: &Detail) -> Span<'static> {
    match &d.guide.validated_commit {
        Some(v) if guide_is_stale(d) => Span::styled(
            format!(
                "[⚠ STALE @{} · HEAD {}]",
                short_id(v),
                short_id(d.head_commit.as_deref().unwrap_or("-"))
            ),
            Style::default()
                .fg(ink(Ink::Warn))
                .add_modifier(Modifier::BOLD),
        ),
        Some(v) => Span::styled(
            format!("[✓ VALID @{}]", short_id(v)),
            Style::default().fg(ink(Ink::Ok)),
        ),
        None => Span::styled(
            "[· NOT VALIDATED]".to_string(),
            Style::default().fg(ink(Ink::Muted)),
        ),
    }
}

fn branch_badge(d: &Detail) -> Span<'static> {
    match &d.branch {
        Some(b) => Span::styled(
            format!("[⎇ {}]", b.branch),
            Style::default().fg(ink(Ink::Info)),
        ),
        None => Span::styled(
            "[⎇ NO BRANCH]".to_string(),
            Style::default().fg(ink(Ink::Muted)),
        ),
    }
}

fn feedback_badge(d: &Detail) -> Vec<Span<'static>> {
    let open: Vec<_> = d
        .annotations
        .iter()
        .filter(|a| a.kind == "comment" && a.status == "open")
        .collect();
    let revise = open.iter().filter(|a| a.request_revision).count();
    let mut text = if open.is_empty() {
        "[· NO FEEDBACK".to_string()
    } else {
        format!("[! {} FEEDBACK", open.len())
    };
    let mut style = Style::default().fg(if open.is_empty() {
        ink(Ink::Muted)
    } else {
        ink(Ink::Accent)
    });
    if revise > 0 {
        text.push_str(&format!(" · ⟳ {revise} REVISE"));
        style = Style::default()
            .fg(ink(Ink::Warn))
            .add_modifier(Modifier::BOLD);
    }
    text.push(']');
    vec![Span::styled(text, style)]
}

fn id_badge(d: &Detail) -> Span<'static> {
    let t = &d.task;
    let id = t.id.map(|i| format!("#{i}")).unwrap_or_else(|| "#-".into());
    Span::styled(
        format!("{id} · {}", short_id(&t.uuid.to_string())),
        Style::default().fg(ink(Ink::Soft)),
    )
}
