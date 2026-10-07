use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::theme::{GLYPH_PROMPT, Ink, Theme, ink};

pub fn too_small(f: &mut Frame, min_w: u16, min_h: u16) -> bool {
    let area = f.area();
    if area.width >= min_w && area.height >= min_h {
        return false;
    }
    let lines = vec![
        Line::from(Span::styled(
            "Terminal too small",
            Style::new().fg(ink(Ink::Warn)).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("need {min_w}×{min_h}, have {}×{}", area.width, area.height),
            Style::new().fg(ink(Ink::Muted)),
        )),
        Line::from(Span::styled(
            "resize, or press q",
            Style::new().fg(ink(Ink::Muted)),
        )),
    ];
    let top = area.height.saturating_sub(lines.len() as u16) / 2;
    let body = Rect {
        y: area.y + top,
        height: area.height - top,
        ..area
    };
    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        body,
    );
    true
}

pub struct Chrome {
    pub header: Rect,
    pub body: Rect,
    pub footer: Rect,
}

pub fn chrome(area: Rect) -> Chrome {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    Chrome {
        header,
        body,
        footer,
    }
}

pub fn render_header(f: &mut Frame, area: Rect, theme: &Theme, title: &str, status: &[Span]) {
    let mut spans = vec![
        Span::styled(" sara ", theme.badge(super::theme::Tone::Accent)),
        Span::styled(format!(" {GLYPH_PROMPT} {title}"), theme.accent()),
    ];
    let left: usize = spans.iter().map(Span::width).sum();
    let right: usize = status.iter().map(Span::width).sum();
    let gap = (area.width as usize).saturating_sub(left + right);
    if gap > 0 && !status.is_empty() {
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(status.iter().cloned());
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn render_footer(f: &mut Frame, area: Rect, theme: &Theme, hints: &[(&str, &str)]) {
    let mut spans = Vec::new();
    for (i, (key, label)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", theme.muted()));
        }
        spans.push(Span::styled(*key, theme.accent()));
        spans.push(Span::styled(format!(" {label}"), theme.muted()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
#[path = "../../../tests/unit/infrastructure/tui/screen.rs"]
mod tests;
