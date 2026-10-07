use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::commands::shared::month_abbr;
use crate::infrastructure::tui::theme::{Ink, heat, ink};

use crate::commands::info::types::Detail;

#[allow(dead_code)]
fn render_project_stats(f: &mut Frame, area: ratatui::layout::Rect, d: &Detail) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Project ")
        .border_style(Style::default().fg(ink(Ink::Muted)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(ref s) = d.stats else {
        return;
    };

    let bar = |count: u32, total: u32, width: usize| -> String {
        if total == 0 {
            return " ".repeat(width);
        }
        let filled = ((count as f64 / total as f64) * width as f64).round() as usize;
        "█".repeat(filled.min(width))
    };

    let total_ever = s.pending + s.completed_total;
    let completion_rate = if total_ever > 0 {
        format!(
            "{:.0}%",
            s.completed_total as f64 / total_ever as f64 * 100.0
        )
    } else {
        "—".to_string()
    };

    let w = inner.width.saturating_sub(2) as usize;
    let bar_w = w.saturating_sub(16).clamp(3, 10);

    let mut lines: Vec<Line> = vec![];

    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<10}", "Pending"),
            Style::default().fg(ink(Ink::Soft)),
        ),
        Span::raw(format!("{:>3}", s.pending)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<10}", "Active"),
            Style::default().fg(ink(Ink::Soft)),
        ),
        Span::styled(
            format!("{:>3}", s.active),
            Style::default().fg(if s.active > 0 {
                ink(Ink::Ok)
            } else {
                ink(Ink::Plain)
            }),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<10}", "Done"),
            Style::default().fg(ink(Ink::Soft)),
        ),
        Span::raw(format!("{:>3}", s.completed_total)),
        Span::styled(
            format!("  {}", completion_rate),
            Style::default().fg(ink(Ink::Muted)),
        ),
    ]));

    lines.push(Line::from(Span::styled(
        "  ─────────────",
        Style::default().fg(ink(Ink::Muted)),
    )));

    let pri_total = s.pending.max(1);
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<5}", "H"),
            Style::default()
                .fg(ink(Ink::Err))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<bar_w$}", bar(s.high, pri_total, bar_w)),
            Style::default().fg(ink(Ink::Err)),
        ),
        Span::styled(format!(" {}", s.high), Style::default().fg(ink(Ink::Muted))),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<5}", "M"), Style::default().fg(ink(Ink::Warn))),
        Span::styled(
            format!("{:<bar_w$}", bar(s.medium, pri_total, bar_w)),
            Style::default().fg(ink(Ink::Warn)),
        ),
        Span::styled(
            format!(" {}", s.medium),
            Style::default().fg(ink(Ink::Muted)),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<5}", "L"), Style::default().fg(ink(Ink::Ok))),
        Span::styled(
            format!("{:<bar_w$}", bar(s.low, pri_total, bar_w)),
            Style::default().fg(ink(Ink::Ok)),
        ),
        Span::styled(format!(" {}", s.low), Style::default().fg(ink(Ink::Muted))),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<5}", "—"),
            Style::default().fg(ink(Ink::Muted)),
        ),
        Span::styled(
            format!("{:<bar_w$}", bar(s.no_pri, pri_total, bar_w)),
            Style::default().fg(ink(Ink::Muted)),
        ),
        Span::styled(
            format!(" {}", s.no_pri),
            Style::default().fg(ink(Ink::Muted)),
        ),
    ]));

    lines.push(Line::from(Span::styled(
        "  ─────────────",
        Style::default().fg(ink(Ink::Muted)),
    )));

    if s.overdue > 0 {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<10}", "Overdue"),
                Style::default().fg(ink(Ink::Err)),
            ),
            Span::styled(
                format!("{:>3}", s.overdue),
                Style::default()
                    .fg(ink(Ink::Err))
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if s.due_today > 0 {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<10}", "Today"),
                Style::default().fg(ink(Ink::Warn)),
            ),
            Span::styled(
                format!("{:>3}", s.due_today),
                Style::default().fg(ink(Ink::Warn)),
            ),
        ]));
    }
    let due_later = s.due_week.saturating_sub(s.due_today);
    if due_later > 0 {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<10}", "This week"),
                Style::default().fg(ink(Ink::Soft)),
            ),
            Span::raw(format!("{:>3}", due_later)),
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

#[allow(dead_code)]
fn render_mini_heatmap(
    f: &mut Frame,
    area: ratatui::layout::Rect,
    counts: &std::collections::HashMap<chrono::NaiveDate, u32>,
    project: &str,
) {
    use chrono::{Datelike, Duration, Local};

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", project))
        .border_style(Style::default().fg(ink(Ink::Muted)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let max = counts.values().copied().max().unwrap_or(1).max(1);
    let today = Local::now().date_naive();

    let days_since_sunday = today.weekday().num_days_from_sunday();
    let grid_end = today - Duration::days(days_since_sunday as i64);

    let cell_w: u16 = 3;
    let label_w: u16 = 4;
    let num_weeks = ((inner.width.saturating_sub(label_w)) / cell_w).clamp(4, 16) as i64;
    let grid_start = grid_end - Duration::weeks(num_weeks) + Duration::days(1);

    {
        let mut spans: Vec<Span> = vec![Span::raw(format!(
            "{:<width$}",
            "",
            width = label_w as usize
        ))];
        let mut last_month = 0u32;
        let mut ws = grid_start;
        for _ in 0..num_weeks {
            let m = ws.month();
            if m != last_month {
                let name = &month_abbr(m)[..3];
                spans.push(Span::styled(
                    format!("{:<width$}", name, width = cell_w as usize),
                    Style::default().fg(ink(Ink::Muted)),
                ));
                last_month = m;
            } else {
                spans.push(Span::raw(format!(
                    "{:<width$}",
                    "",
                    width = cell_w as usize
                )));
            }
            ws += Duration::weeks(1);
        }
        let month_area = ratatui::layout::Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };
        f.render_widget(Paragraph::new(Line::from(spans)), month_area);
    }

    const DAY_LABELS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const SHOW_LABEL: [bool; 7] = [false, true, false, true, false, true, false];

    for row in 0..7u32 {
        if inner.y + 1 + row as u16 >= inner.y + inner.height {
            break;
        }
        let mut spans: Vec<Span> = vec![];
        let label = if SHOW_LABEL[row as usize] {
            DAY_LABELS[row as usize]
        } else {
            "   "
        };
        spans.push(Span::styled(
            format!("{label} "),
            Style::default().fg(ink(Ink::Muted)),
        ));

        let mut ws = grid_start;
        for _ in 0..num_weeks {
            let day = ws + Duration::days(row as i64);
            let in_future = day > today;
            let count = if in_future {
                0
            } else {
                counts.get(&day).copied().unwrap_or(0)
            };
            let color = if in_future {
                ink(Ink::Void)
            } else {
                heat(count, max)
            };
            spans.push(Span::styled("██ ", Style::default().bg(color).fg(color)));
            ws += Duration::weeks(1);
        }

        let row_area = ratatui::layout::Rect {
            x: inner.x,
            y: inner.y + 1 + row as u16,
            width: inner.width,
            height: 1,
        };
        f.render_widget(Paragraph::new(Line::from(spans)), row_area);
    }

    let total: u32 = counts.values().sum();
    let stats_area = ratatui::layout::Rect {
        x: inner.x,
        y: inner.y + 8,
        width: inner.width,
        height: 1,
    };
    if stats_area.y < inner.y + inner.height {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("  {total} events (16w)"),
                Style::default().fg(ink(Ink::Muted)),
            ))),
            stats_area,
        );
    }
}
