use chrono::{Datelike, Duration, NaiveDate};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::types::{ActivityState, Drill};
use crate::commands::shared::{month_abbr, truncate};
use crate::infrastructure::tui::screen;
use crate::infrastructure::tui::theme::{GLYPH_DONE, GLYPH_OPEN, Ink, Theme, heat, ink};

const DOT: &str = "●";
const EMPTY: &str = "·";
const LABEL_W: usize = 4;
const CELL_W: usize = 2;

pub(super) const MIN_SIZE: (u16, u16) = (50, 18);

pub(super) const FOOTER: [(&str, &str); 5] = [
    ("h/l", "week"),
    ("j/k", "day"),
    ("↵", "tasks"),
    ("Esc", "close"),
    ("q", "quit"),
];

pub(super) fn render(f: &mut Frame, st: &ActivityState) {
    if screen::too_small(f, MIN_SIZE.0, MIN_SIZE.1) {
        return;
    }
    let theme = Theme::detect();
    let c = screen::chrome(f.area());
    let scope = st.data.project.as_deref().unwrap_or("all projects");
    let status = vec![
        Span::styled(format!("{scope} · "), theme.muted()),
        Span::styled(format!("{}d streak ", st.data.cur_streak), theme.ok()),
    ];
    screen::render_header(f, c.header, &theme, "activity", &status);

    let [stats, _, grid, _, day] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(10),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(c.body);
    render_stats(f, st, &theme, stats);
    render_grid(f, st, &theme, grid);
    render_day(f, st, &theme, day);
    screen::render_footer(f, c.footer, &theme, &FOOTER);
}

fn section(theme: &Theme, title: String, note: String) -> Line<'static> {
    Line::from(vec![
        Span::styled("▌ ", theme.accent()),
        Span::styled(title, theme.accent()),
        Span::styled(format!("  {note}"), theme.muted()),
    ])
}

fn render_stats(f: &mut Frame, st: &ActivityState, theme: &Theme, area: Rect) {
    let d = &st.data;
    let rate = (d.total_completed * 100)
        .checked_div(d.total_created)
        .map(|r| format!("{r}%"))
        .unwrap_or_else(|| "—".into());
    let value = Style::default()
        .fg(ink(Ink::Text))
        .add_modifier(Modifier::BOLD);
    let mut spans = vec![Span::raw("  ")];
    let stats = [
        (d.total_created.to_string(), "created"),
        (d.total_completed.to_string(), "completed"),
        (rate, "done rate"),
        (format!("{}d", d.cur_streak), "streak"),
        (format!("{}d", d.longest_streak), "best"),
    ];
    for (i, (v, label)) in stats.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ·  ", theme.muted()));
        }
        spans.push(Span::styled(v, value));
        spans.push(Span::styled(format!("\u{a0}{label}"), theme.muted()));
    }
    f.render_widget(
        Paragraph::new(vec![
            section(theme, "Overview".into(), String::new()),
            Line::from(spans),
        ]),
        area,
    );
}

pub(super) fn week_start(day: NaiveDate) -> NaiveDate {
    day - Duration::days(day.weekday().num_days_from_sunday() as i64)
}

pub(super) fn grid_start(st: &ActivityState, weeks: i64) -> NaiveDate {
    let latest = week_start(st.today) - Duration::weeks(weeks - 1);
    latest.min(week_start(st.cursor))
}

fn render_grid(f: &mut Frame, st: &ActivityState, theme: &Theme, area: Rect) {
    let weeks = ((area.width as usize).saturating_sub(LABEL_W) / CELL_W).clamp(4, 53) as i64;
    let start = grid_start(st, weeks);
    let max = st.data.counts.values().copied().max().unwrap_or(1).max(1);

    let mut lines = vec![section(theme, format!("Last {weeks} weeks"), String::new())];

    let mut months = " ".repeat(LABEL_W);
    let mut last = 0;
    for w in 0..weeks {
        let first = start + Duration::weeks(w);
        let col = LABEL_W + w as usize * CELL_W;
        if first.month() != last && months.chars().count() <= col {
            months.push_str(&" ".repeat(col - months.chars().count()));
            months.push_str(month_abbr(first.month()));
            last = first.month();
        }
    }
    lines.push(Line::from(Span::styled(
        months,
        Style::default().fg(ink(Ink::Soft)),
    )));

    const DAYS: [&str; 7] = ["", "Mon", "", "Wed", "", "Fri", ""];
    for (row, label) in DAYS.iter().enumerate() {
        let mut spans = vec![Span::styled(format!("{label:<LABEL_W$}"), theme.muted())];
        for w in 0..weeks {
            let day = start + Duration::weeks(w) + Duration::days(row as i64);
            spans.push(cell(st, theme, day, max));
            spans.push(Span::raw(" "));
        }
        lines.push(Line::from(spans));
    }

    let mut legend = vec![Span::styled(format!("{:LABEL_W$}less ", ""), theme.muted())];
    for n in [0, 1, 2, 3, 4] {
        let glyph = if n == 0 { EMPTY } else { DOT };
        legend.push(Span::styled(
            format!("{glyph} "),
            Style::default().fg(heat(n, 4)),
        ));
    }
    legend.push(Span::styled("more", theme.muted()));
    lines.push(Line::from(legend));

    f.render_widget(Paragraph::new(lines), area);
}

fn cell(st: &ActivityState, theme: &Theme, day: NaiveDate, max: u32) -> Span<'static> {
    if day > st.today {
        return Span::raw(" ");
    }
    let n = st.count(day);
    let glyph = if n == 0 { EMPTY } else { DOT };
    let mut style = Style::default().fg(heat(n, max));
    if day == st.cursor {
        style = if theme.color {
            style.bg(ink(Ink::Select)).add_modifier(Modifier::BOLD)
        } else {
            style.add_modifier(Modifier::REVERSED)
        };
    }
    Span::styled(glyph, style)
}

fn day_title(day: NaiveDate) -> String {
    format!(
        "{} {} {} {}",
        day.weekday(),
        day.day(),
        month_abbr(day.month()),
        day.year()
    )
}

fn render_day(f: &mut Frame, st: &ActivityState, theme: &Theme, area: Rect) {
    let n = st.count(st.cursor);
    let mut lines = Vec::new();
    match &st.drill {
        None => {
            lines.push(section(
                theme,
                day_title(st.cursor),
                format!("{n} events · ↵ to see the tasks touched"),
            ));
        }
        Some(Drill { day, tasks }) => {
            lines.push(section(
                theme,
                day_title(*day),
                format!("{} tasks touched", tasks.len()),
            ));
            if tasks.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  Nothing touched on this day.",
                    theme.muted(),
                )));
            }
            let room = (area.height as usize).saturating_sub(1);
            let width = area.width as usize;
            for t in tasks.iter().take(room) {
                let done = t.status == "completed";
                let (glyph, tone) = if done {
                    (GLYPH_DONE, theme.ok())
                } else {
                    (GLYPH_OPEN, theme.accent())
                };
                let id = t.id.map(|i| format!("#{i}")).unwrap_or_default();
                let project = if st.data.project.is_none() {
                    format!("  {}", t.project)
                } else {
                    String::new()
                };
                let title_w = width.saturating_sub(12 + project.chars().count());
                lines.push(Line::from(vec![
                    Span::styled(format!("  {glyph}\u{a0}"), tone),
                    Span::styled(format!("{id:>5}  "), theme.muted()),
                    Span::styled(
                        truncate(&t.title, title_w),
                        Style::default().fg(ink(Ink::Text)),
                    ),
                    Span::styled(project, theme.muted()),
                ]));
            }
            if tasks.len() > room {
                lines.pop();
                lines.push(Line::from(Span::styled(
                    format!("  … {} more", tasks.len() - room + 1),
                    theme.muted(),
                )));
            }
        }
    }
    f.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/activity/render.rs"]
mod tests;
