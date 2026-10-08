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

pub(super) fn collapsed_text(s: &str, verbose: bool) -> String {
    const COLLAPSED_CHARS: usize = 160;
    if verbose || s.chars().count() <= COLLAPSED_CHARS {
        return s.to_string();
    }
    let t: String = s.chars().take(COLLAPSED_CHARS).collect();
    format!("{t}…  (v to expand)")
}

pub(super) fn label_line(
    glyph: &str,
    title: &str,
    count: Option<usize>,
    width: usize,
    selected: bool,
) -> Line<'static> {
    let mut head = Style::default()
        .fg(ink(Ink::Accent))
        .add_modifier(Modifier::BOLD);
    let mut count_style = Style::default().fg(ink(Ink::Soft));
    if selected {
        head = head.fg(ink(Ink::Text)).bg(ink(Ink::Select));
        count_style = count_style.fg(ink(Ink::Text)).bg(ink(Ink::Select));
    }
    let mut spans = vec![Span::styled(
        format!(" {glyph} {}", title.to_uppercase()),
        head,
    )];
    if let Some(n) = count {
        spans.push(Span::styled(format!("  {n}"), count_style));
    }
    let used: usize = spans.iter().map(|s| s.width()).sum();
    let rule = width.saturating_sub(used + 1);
    if rule > 0 {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            "─".repeat(rule),
            Style::default().fg(ink(Ink::Muted)),
        ));
    }
    Line::from(spans)
}

pub(super) fn bar(done: usize, total: usize, cells: usize) -> Vec<Span<'static>> {
    let filled = (done * cells).checked_div(total).unwrap_or(0).min(cells);
    vec![
        Span::styled("▰".repeat(filled), Style::default().fg(ink(Ink::Ok))),
        Span::styled(
            "▱".repeat(cells - filled),
            Style::default().fg(ink(Ink::Muted)),
        ),
    ]
}

pub(super) fn feedback_mark(open: usize, revise: bool, bg: Color) -> Vec<Span<'static>> {
    let mut spans = vec![];
    if open > 0 {
        spans.push(Span::styled(
            format!("  ! {open}"),
            Style::default()
                .fg(ink(Ink::Accent))
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if revise {
        spans.push(Span::styled(
            " ⟳",
            Style::default().fg(ink(Ink::Warn)).bg(bg),
        ));
    }
    spans
}

pub(super) fn wrap_hanging(
    lines: Vec<Line<'static>>,
    width: usize,
    sel: Option<(usize, usize)>,
) -> (Vec<Line<'static>>, Option<(usize, usize)>) {
    let width = width.max(8);
    let mut out = Vec::with_capacity(lines.len());
    let mut starts = Vec::with_capacity(lines.len());
    let mut ends = Vec::with_capacity(lines.len());
    for line in lines {
        starts.push(out.len());
        out.extend(wrap_line(line, width));
        ends.push(out.len() - 1);
    }
    let sel = sel.and_then(|(a, b)| Some((*starts.get(a)?, *ends.get(b)?)));
    (out, sel)
}

fn hanging_indent(text: &str) -> usize {
    let mut col = 0;
    for ch in text.chars() {
        if ch.is_alphanumeric() || matches!(ch, '"' | '\'' | '(' | '[' | '/' | '.' | '~') {
            return col;
        }
        col += Span::raw(ch.to_string()).width();
    }
    0
}

fn pieces(s: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut start = 0;
    let mut in_space = None;
    for (i, ch) in s.char_indices() {
        let sp = ch == ' ';
        if in_space.is_some_and(|prev| prev != sp) {
            out.push(&s[start..i]);
            start = i;
        }
        in_space = Some(sp);
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if line.width() <= width {
        return vec![line];
    }
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    let indent = hanging_indent(&text).min(width / 2);
    let mut out: Vec<Line<'static>> = vec![];
    let mut cur: Vec<Span<'static>> = vec![];
    let mut used = 0usize;
    let mut fresh = false;
    let flush = |cur: &mut Vec<Span<'static>>, out: &mut Vec<Line<'static>>| {
        while cur
            .last()
            .is_some_and(|s| !s.content.is_empty() && s.content.trim().is_empty())
        {
            cur.pop();
        }
        out.push(Line::from(std::mem::take(cur)).style(line.style));
    };
    for span in &line.spans {
        for piece in pieces(&span.content) {
            let is_space = piece.starts_with(' ');
            if fresh && is_space {
                continue;
            }
            let w = Span::raw(piece).width();
            if used + w > width && used > indent && !fresh {
                flush(&mut cur, &mut out);
                cur.push(Span::raw(" ".repeat(indent)));
                used = indent;
                fresh = true;
                if is_space {
                    continue;
                }
            }
            if used + w <= width {
                cur.push(Span::styled(piece.to_string(), span.style));
                used += w;
                fresh = false;
                continue;
            }
            let mut chunk = String::new();
            for ch in piece.chars() {
                let cw = Span::raw(ch.to_string()).width();
                if used + cw > width {
                    if !chunk.is_empty() {
                        cur.push(Span::styled(std::mem::take(&mut chunk), span.style));
                    }
                    flush(&mut cur, &mut out);
                    cur.push(Span::raw(" ".repeat(indent)));
                    used = indent;
                }
                chunk.push(ch);
                used += cw;
            }
            if !chunk.is_empty() {
                cur.push(Span::styled(chunk, span.style));
            }
            fresh = false;
        }
    }
    if !cur.is_empty() {
        flush(&mut cur, &mut out);
    }
    out
}
