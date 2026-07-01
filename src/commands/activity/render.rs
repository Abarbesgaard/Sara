use chrono::{Datelike, Duration, Local, NaiveDate};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::types::ActivityData;

const CELL: &str = "██";

pub(super) fn render(f: &mut Frame, data: &ActivityData) {
    let area = f.area();
    let max = data.counts.values().copied().max().unwrap_or(1).max(1);

    let title = match &data.project {
        Some(p) => format!(" Activity — {p} "),
        None => " Activity — all projects ".to_string(),
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
            Constraint::Length(7), // heatmap (7 rows = Sun–Sat)
            Constraint::Length(2), // legend
            Constraint::Min(1),    // spacer
        ])
        .split(inner);

    let today = Local::now().date_naive();
    let days_since_sunday = today.weekday().num_days_from_sunday();
    let grid_end = today - Duration::days(days_since_sunday as i64);
    let cell_width = CELL.len() as u16 + 1;
    let label_width: u16 = 4;
    let available_width = area.width.saturating_sub(label_width + 2);
    let num_weeks = ((available_width / cell_width) as i64).clamp(4, 52);
    let grid_start = grid_end - Duration::weeks(num_weeks) + Duration::days(1);

    render_stats(f, data, chunks[0]);
    render_month_labels(f, grid_start, num_weeks, cell_width, label_width, chunks[1]);
    render_heatmap(
        f,
        &data.counts,
        today,
        grid_start,
        num_weeks,
        max,
        chunks[2],
    );
    render_legend(f, max, chunks[3]);
}

fn render_stats(f: &mut Frame, data: &ActivityData, area: ratatui::layout::Rect) {
    let rate = if data.total_created > 0 {
        format!(
            "{:.0}%",
            data.total_completed as f64 / data.total_created as f64 * 100.0
        )
    } else {
        "—".to_string()
    };
    let line = Line::from(vec![
        stat_span("Created", &data.total_created.to_string()),
        Span::raw("   "),
        stat_span("Completed", &data.total_completed.to_string()),
        Span::raw("   "),
        stat_span("Completion rate", &rate),
        Span::raw("   "),
        stat_span("Current streak", &format!("{}d", data.cur_streak)),
        Span::raw("   "),
        stat_span("Longest streak", &format!("{}d", data.longest_streak)),
    ]);
    f.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        area,
    );
}

fn render_month_labels(
    f: &mut Frame,
    grid_start: NaiveDate,
    num_weeks: i64,
    cell_width: u16,
    label_width: u16,
    area: ratatui::layout::Rect,
) {
    let mut spans: Vec<Span> = vec![Span::raw(format!(
        "{:<width$}",
        "",
        width = label_width as usize
    ))];
    let mut last_month = 0u32;
    let mut week_start = grid_start;
    for _ in 0..num_weeks {
        let month = week_start.month();
        if month != last_month {
            spans.push(Span::styled(
                format!("{:<width$}", month_abbr(month), width = cell_width as usize),
                Style::default().fg(Color::Gray),
            ));
            last_month = month;
        } else {
            spans.push(Span::raw(format!(
                "{:<width$}",
                "",
                width = cell_width as usize
            )));
        }
        week_start += Duration::weeks(1);
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_heatmap(
    f: &mut Frame,
    counts: &std::collections::HashMap<NaiveDate, u32>,
    today: NaiveDate,
    grid_start: NaiveDate,
    num_weeks: i64,
    max: u32,
    area: ratatui::layout::Rect,
) {
    const DAY_LABELS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const SHOW_LABEL: [bool; 7] = [false, true, false, true, false, true, false];

    for row in 0..7u32 {
        let label = if SHOW_LABEL[row as usize] {
            DAY_LABELS[row as usize]
        } else {
            "   "
        };
        let mut spans = vec![Span::styled(
            format!("{label} "),
            Style::default().fg(Color::DarkGray),
        )];

        let mut week_start = grid_start;
        for _ in 0..num_weeks {
            let day = week_start + Duration::days(row as i64);
            let count = if day > today {
                0
            } else {
                counts.get(&day).copied().unwrap_or(0)
            };
            let color = if day > today {
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

        f.render_widget(
            Paragraph::new(Line::from(spans)),
            ratatui::layout::Rect {
                x: area.x,
                y: area.y + row as u16,
                width: area.width,
                height: 1,
            },
        );
    }
}

fn render_legend(f: &mut Frame, max: u32, area: ratatui::layout::Rect) {
    let levels = [
        (0u32, "none"),
        (1, "low"),
        (3, "med"),
        (6, "high"),
        (max, "peak"),
    ];
    let mut spans = vec![Span::styled(
        "    Less ",
        Style::default().fg(Color::DarkGray),
    )];
    for (count, _) in &levels {
        let color = heat_color(*count, max);
        spans.push(Span::styled(CELL, Style::default().bg(color).fg(color)));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled("More", Style::default().fg(Color::DarkGray)));
    spans.push(Span::styled(
        "    q/Esc to close",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn heat_color(count: u32, max: u32) -> Color {
    if count == 0 {
        return Color::Rgb(22, 27, 34);
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

fn month_abbr(m: u32) -> &'static str {
    match m {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    }
}
