use chrono::{Datelike, Duration, Local, NaiveDate};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::collections::HashMap;

use super::month_abbr;

const CELL: &str = "██"; // two-wide block per day

pub(super) fn render(
    f: &mut Frame,
    counts: &HashMap<NaiveDate, u32>,
    project: Option<&str>,
    total_created: u32,
    total_completed: u32,
    cur_streak: u32,
    longest_streak: u32,
) {
    let area = f.area();
    let max = counts.values().copied().max().unwrap_or(1).max(1);

    let title = if let Some(p) = project {
        format!(" Activity — {p} ")
    } else {
        " Activity — all projects ".to_string()
    };

    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // stats bar
            Constraint::Length(2), // month labels
            Constraint::Length(7), // heatmap (7 rows = Mon–Sun)
            Constraint::Length(2), // legend
            Constraint::Min(1),    // spacer / footer
        ])
        .split(inner);

    // ── Stats bar ────────────────────────────────────────────────────────────
    let rate = if total_created > 0 {
        format!(
            "{:.0}%",
            total_completed as f64 / total_created as f64 * 100.0
        )
    } else {
        "—".to_string()
    };
    let stats_line = Line::from(vec![
        stat_span("Created", &total_created.to_string()),
        Span::raw("   "),
        stat_span("Completed", &total_completed.to_string()),
        Span::raw("   "),
        stat_span("Completion rate", &rate),
        Span::raw("   "),
        stat_span("Current streak", &format!("{cur_streak}d")),
        Span::raw("   "),
        stat_span("Longest streak", &format!("{longest_streak}d")),
    ]);
    f.render_widget(
        Paragraph::new(stats_line).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        chunks[0],
    );

    // ── Build the 52-week grid ────────────────────────────────────────────────
    // Align to the most recent Sunday so weeks run Sun→Sat (like GitHub).
    let today = Local::now().date_naive();
    // Find the most recent Sunday on or before today
    let days_since_sunday = today.weekday().num_days_from_sunday();
    let grid_end = today - Duration::days(days_since_sunday as i64); // last Sunday

    // How many weeks fit in the available width?
    let cell_width = CELL.len() as u16 + 1; // "██ "
    let label_width: u16 = 4; // "Mon " etc.
    let available_width = area.width.saturating_sub(label_width + 2);
    let num_weeks = ((available_width / cell_width) as i64).clamp(4, 52);

    let grid_start = grid_end - Duration::weeks(num_weeks) + Duration::days(1);

    // month_labels: for each week column, what month starts in that column
    let mut month_label_line: Vec<Span> = vec![Span::raw(format!(
        "{:<width$}",
        "",
        width = label_width as usize
    ))];
    {
        let mut last_month = 0u32;
        let mut week_start = grid_start;
        for _col in 0..num_weeks {
            let month = week_start.month();
            if month != last_month {
                let name = month_abbr(month);
                month_label_line.push(Span::styled(
                    format!("{:<width$}", name, width = cell_width as usize),
                    Style::default().fg(Color::Gray),
                ));
                last_month = month;
            } else {
                month_label_line.push(Span::raw(format!(
                    "{:<width$}",
                    "",
                    width = cell_width as usize
                )));
            }
            week_start += Duration::weeks(1);
        }
    }
    f.render_widget(Paragraph::new(Line::from(month_label_line)), chunks[1]);

    // Heatmap: 7 rows (weekday), num_weeks columns
    const DAY_LABELS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const SHOW_LABEL: [bool; 7] = [false, true, false, true, false, true, false];

    for row in 0..7u32 {
        let mut spans: Vec<Span> = vec![];
        // Day label (3 chars + space)
        let label = if SHOW_LABEL[row as usize] {
            DAY_LABELS[row as usize]
        } else {
            "   "
        };
        spans.push(Span::styled(
            format!("{label} "),
            Style::default().fg(Color::DarkGray),
        ));

        let mut week_start = grid_start;
        for _col in 0..num_weeks {
            // The day in this cell: week_start + row (0=Sun … 6=Sat)
            // grid_start is a Sunday, so offset by row days.
            let day = week_start + Duration::days(row as i64);
            let in_future = day > today;
            let count = if in_future {
                0
            } else {
                counts.get(&day).copied().unwrap_or(0)
            };

            let color = if in_future {
                Color::Rgb(12, 14, 18)
            } else {
                heat_color(count, max)
            };
            spans.push(Span::styled(
                format!("{CELL} "),
                Style::default().bg(color).fg(color),
            ));
            week_start += Duration::weeks(1);
        }

        // Render this row into a sub-rect of the heatmap chunk
        let row_area = ratatui::layout::Rect {
            x: chunks[2].x,
            y: chunks[2].y + row as u16,
            width: chunks[2].width,
            height: 1,
        };
        f.render_widget(Paragraph::new(Line::from(spans)), row_area);
    }

    // ── Legend ────────────────────────────────────────────────────────────────
    let legend_levels = [
        (0u32, "none"),
        (1, "low"),
        (3, "med"),
        (6, "high"),
        (max, "peak"),
    ];
    let mut legend_spans: Vec<Span> = vec![Span::styled(
        "    Less ",
        Style::default().fg(Color::DarkGray),
    )];
    for (count, _) in &legend_levels {
        let color = heat_color(*count, max);
        legend_spans.push(Span::styled(CELL, Style::default().bg(color).fg(color)));
        legend_spans.push(Span::raw(" "));
    }
    legend_spans.push(Span::styled("More", Style::default().fg(Color::DarkGray)));
    legend_spans.push(Span::styled(
        "    q/Esc to close",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
    ));
    f.render_widget(Paragraph::new(Line::from(legend_spans)), chunks[3]);
}

fn heat_color(count: u32, max: u32) -> Color {
    if count == 0 {
        return Color::Rgb(22, 27, 34); // empty cell
    }
    let ratio = count as f64 / max.max(1) as f64;
    if ratio < 0.25 {
        Color::Rgb(14, 68, 41)
    } else if ratio < 0.5 {
        Color::Rgb(0, 109, 50)
    } else if ratio < 0.75 {
        Color::Rgb(38, 166, 65)
    } else {
        Color::Rgb(57, 211, 83)
    }
}

fn stat_span(label: &str, value: &str) -> Span<'static> {
    Span::raw(format!("{label}: {value}")).style(Style::default().fg(Color::White))
}
