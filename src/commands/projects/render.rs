use crate::infrastructure::tui::screen;
use crate::infrastructure::tui::theme::{Ink, Theme, ink};
use anyhow::Result;
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::{ProjectAction, ProjectListState, ProjectRow};
use crate::commands::shared::{rel_time, truncate};
use crate::infrastructure::tui::keymap::{Action, KeyDispatcher, Mode};

pub(super) fn list_loop<B: Backend<Error: Send + Sync + 'static>>(
    terminal: &mut Terminal<B>,
    st: &mut ProjectListState,
) -> Result<ProjectAction> {
    let mut keys = KeyDispatcher::new();
    loop {
        let size = terminal.size()?;
        let viewport = size.height.saturating_sub(4);
        crate::infrastructure::tui::scroll_into_view(&mut st.scroll, st.selected as u16, viewport);

        let lines = build_lines(st);
        terminal.draw(|f| render(f, st, &lines))?;

        let Some(key) = crate::infrastructure::tui::next_key(100)? else {
            continue;
        };
        if let Some(action) = apply(st, keys.dispatch(key, Mode::Normal)) {
            return Ok(action);
        }
    }
}

pub(super) fn apply(st: &mut ProjectListState, action: Action) -> Option<ProjectAction> {
    let last = st.rows.len().saturating_sub(1);
    match action {
        Action::Quit | Action::Cancel => return Some(ProjectAction::Quit),
        Action::Down => st.selected = (st.selected + 1).min(last),
        Action::Up => st.selected = st.selected.saturating_sub(1),
        Action::Top => st.selected = 0,
        Action::Bottom => st.selected = last,
        Action::PageDown => st.scroll = st.scroll.saturating_add(10),
        Action::PageUp => st.scroll = st.scroll.saturating_sub(10),
        Action::Confirm => {
            if let Some(row) = st.rows.get(st.selected) {
                return Some(ProjectAction::Open(row.name.clone()));
            }
        }
        _ => {}
    }
    None
}

fn build_lines(st: &ProjectListState) -> Vec<Line<'static>> {
    let name_w = st
        .rows
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(4)
        .clamp(4, 28);
    let badge_w = st
        .rows
        .iter()
        .map(|r| badges(r).iter().map(Span::width).sum::<usize>())
        .max()
        .unwrap_or(0);
    st.rows
        .iter()
        .enumerate()
        .map(|(i, r)| project_line(r, i == st.selected, name_w, badge_w))
        .collect()
}

fn badge(n: u32, label: &str, tone: Ink) -> Span<'static> {
    let fg = if n == 0 { ink(Ink::Muted) } else { ink(tone) };
    Span::styled(format!("[{n}\u{a0}{label}]"), Style::default().fg(fg))
}

pub(super) fn badges(r: &ProjectRow) -> Vec<Span<'static>> {
    let mut out = vec![
        badge(r.pending, "open", Ink::Accent),
        Span::raw(" "),
        badge(r.active, "active", Ink::Ok),
    ];
    if r.stale > 0 {
        out.push(Span::raw(" "));
        out.push(badge(r.stale, "stale", Ink::Warn));
    }
    if r.feedback > 0 {
        out.push(Span::raw(" "));
        out.push(badge(r.feedback, "feedback", Ink::Err));
    }
    out
}

fn project_line(r: &ProjectRow, is_sel: bool, name_w: usize, badge_w: usize) -> Line<'static> {
    let prefix = if is_sel { " ▶ " } else { "   " };
    let name = format!("{:<width$}", truncate(&r.name, name_w), width = name_w);

    let mut meta = String::new();
    if let Some(g) = r.goal.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        meta.push_str(&truncate(g, 48));
    }
    if let Some(s) = r.stack.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !meta.is_empty() {
            meta.push_str(" · ");
        }
        meta.push_str(&format!("[{}]", truncate(s, 24)));
    }
    let activity = r.last_activity.map(rel_time).unwrap_or_default();

    let mut spans = vec![Span::styled(
        format!("{prefix}{name}  "),
        Style::default()
            .fg(ink(if is_sel { Ink::Text } else { Ink::Accent }))
            .add_modifier(Modifier::BOLD),
    )];
    let row_badges = badges(r);
    let used: usize = row_badges.iter().map(Span::width).sum();
    spans.extend(row_badges);
    spans.push(Span::raw(" ".repeat(badge_w - used)));
    spans.push(Span::styled(
        format!("   {:>4}\u{a0}done", r.done),
        Style::default().fg(ink(Ink::Muted)),
    ));
    spans.push(Span::styled(
        format!("   {meta}"),
        Style::default().fg(ink(Ink::Soft)),
    ));
    spans.push(Span::styled(
        format!("   {activity}"),
        Style::default().fg(ink(Ink::Muted)),
    ));
    if is_sel {
        for s in &mut spans {
            s.style = s.style.bg(ink(Ink::Select));
        }
    }
    Line::from(spans)
}

pub(super) const MIN_SIZE: (u16, u16) = (40, 6);

pub(super) const FOOTER: [(&str, &str); 4] = [
    ("j/k", "move"),
    ("↵", "open board"),
    ("PgDn/PgUp", "scroll"),
    ("q", "quit"),
];

fn render(f: &mut Frame, st: &ProjectListState, lines: &[Line]) {
    if screen::too_small(f, MIN_SIZE.0, MIN_SIZE.1) {
        return;
    }
    let theme = Theme::detect();
    let c = screen::chrome(f.area());
    let open: u32 = st.rows.iter().map(|r| r.pending).sum();
    let active: u32 = st.rows.iter().map(|r| r.active).sum();
    let status = vec![
        Span::styled(format!("{} projects · ", st.rows.len()), theme.muted()),
        Span::styled(format!("{open} open"), theme.accent()),
        Span::styled(" · ", theme.muted()),
        Span::styled(format!("{active} active "), theme.ok()),
    ];
    screen::render_header(f, c.header, &theme, "projects", &status);

    let [label, list] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(c.body);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("▌ ", theme.accent()),
            Span::styled("Projects", theme.accent()),
            Span::styled("  by recent activity", theme.muted()),
        ])),
        label,
    );
    f.render_widget(Paragraph::new(lines.to_vec()).scroll((st.scroll, 0)), list);
    screen::render_footer(f, c.footer, &theme, &FOOTER);
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/projects/render.rs"]
mod tests;
