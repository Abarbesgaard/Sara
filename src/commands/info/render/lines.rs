use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::infrastructure::tui::theme::{Ink, ink};

pub(super) fn nav_line<'a>(text: &str, color: Color, italic: bool, selected: bool) -> Line<'a> {
    let mut style = Style::default().fg(color);
    if italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if selected {
        style = style
            .bg(ink(Ink::Select))
            .fg(ink(Ink::Text))
            .add_modifier(Modifier::BOLD);
    }
    let prefix = if selected { " ▶ " } else { "   " };
    Line::from(vec![
        Span::styled(prefix.to_string(), style),
        Span::styled(text.to_string(), style),
    ])
}

#[allow(dead_code)]
pub(super) fn sel_line<'a>(spans: Vec<Span<'a>>, selected: bool) -> Line<'a> {
    if !selected {
        return Line::from(spans);
    }
    let highlighted: Vec<Span> = spans
        .into_iter()
        .map(|s| Span::styled(s.content, s.style.bg(ink(Ink::Select)).fg(ink(Ink::Text))))
        .collect();
    Line::from(highlighted)
}

pub(super) fn collapsed_text(s: &str, verbose: bool) -> String {
    const COLLAPSED_CHARS: usize = 160;
    if verbose || s.chars().count() <= COLLAPSED_CHARS {
        return s.to_string();
    }
    let t: String = s.chars().take(COLLAPSED_CHARS).collect();
    format!("{t}…  (v to expand)")
}

pub(super) fn key_span(k: &str) -> Span<'static> {
    Span::styled(format!("  {:<12}", k), Style::default().fg(ink(Ink::Muted)))
}

pub(super) fn field_line<'a>(k: &str, v: &str) -> Line<'a> {
    Line::from(vec![key_span(k), Span::raw(v.to_string())])
}

pub(super) fn section(k: &str) -> Line<'static> {
    Line::from(Span::styled(
        k.to_string(),
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(ink(Ink::Accent)),
    ))
}
