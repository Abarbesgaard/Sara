use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::infrastructure::db::{FlowEvent, FlowKind};
use crate::infrastructure::tui::screen::{chrome, render_footer, render_header};
use crate::infrastructure::tui::theme::{
    GLYPH_CURRENT, GLYPH_DONE, GLYPH_OPEN, GLYPH_PROMPT, Theme, Tone,
};

use super::state::{App, Card, Mode, Pulse, in_feed, is_minimal};

pub fn render(f: &mut Frame, app: &App, theme: &Theme) {
    let area = f.area();
    let minimal = is_minimal(app.minimal, area.width, area.height);
    match (app.mode, &app.focus) {
        (Mode::Task(_), Some(focus)) if minimal => {
            let feed: Vec<(Option<String>, &FlowEvent)> = focus
                .events
                .iter()
                .filter(|e| in_feed(e))
                .map(|e| (None, e))
                .collect();
            render_minimal(f, area, &[&focus.card], &feed, app.now, theme)
        }
        (Mode::Task(_), Some(focus)) => {
            render_task(f, area, app, &focus.card, &focus.events, theme)
        }
        (Mode::Task(_), None) => {}
        (Mode::Mission, _) if minimal => {
            let cards = app.visible_cards(true);
            let many = cards.len() > 1;
            let grouped = is_grouped(&cards);
            let feed: Vec<(Option<String>, &FlowEvent)> = app
                .visible_feed(true)
                .into_iter()
                .map(|i| {
                    let label = if grouped {
                        Some(format!("{} {}", i.project, i.label))
                    } else {
                        many.then(|| i.label.clone())
                    };
                    (label, &i.event)
                })
                .collect();
            render_minimal(f, area, &cards, &feed, app.now, theme)
        }
        (Mode::Mission, _) => render_mission(f, area, app, theme),
    }
}

pub fn age(now: DateTime<Utc>, at: DateTime<Utc>) -> String {
    let s = (now - at).num_seconds().max(0);
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

fn fit(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(width - 1).collect();
    out.truncate(out.trim_end().len());
    out.push('…');
    out
}

fn pulse_tone(p: Pulse) -> Tone {
    match p {
        Pulse::Live => Tone::Ok,
        Pulse::Stalled => Tone::Warn,
        Pulse::Idle => Tone::Muted,
    }
}

fn pulse_badge(p: Pulse, theme: &Theme) -> Span<'static> {
    let text = match p {
        Pulse::Live => " LIVE ",
        Pulse::Stalled => " STALLED ",
        Pulse::Idle => " IDLE ",
    };
    Span::styled(text, theme.badge(pulse_tone(p)))
}

pub fn rail(card: &Card, width: usize, theme: &Theme) -> Vec<Span<'static>> {
    let n = card.steps.len();
    if n == 0 || width == 0 {
        return Vec::new();
    }
    let done = card.done_count();
    let current = card.current();
    let glyphs: Vec<&str> = if n <= width {
        card.steps
            .iter()
            .enumerate()
            .map(|(i, s)| match (s.done, Some(i) == current) {
                (true, _) => GLYPH_DONE,
                (false, true) => GLYPH_CURRENT,
                (false, false) => GLYPH_OPEN,
            })
            .collect()
    } else {
        let filled = (done * width / n).min(width);
        let mut g = vec![GLYPH_DONE; filled];
        if current.is_some() && filled < width {
            g.push(GLYPH_CURRENT);
        }
        g.resize(width, GLYPH_OPEN);
        g
    };
    glyphs
        .into_iter()
        .map(|g| {
            let style = match g {
                GLYPH_DONE => theme.ok(),
                GLYPH_CURRENT => theme.accent(),
                _ => theme.muted(),
            };
            Span::styled(g, style)
        })
        .collect()
}

fn label_style(card: &Card, theme: &Theme) -> Style {
    match card.pulse {
        Pulse::Live => theme.accent(),
        Pulse::Stalled => theme.warn(),
        Pulse::Idle => theme.muted(),
    }
}

fn card_rail_line(card: &Card, width: usize, indent: &str, theme: &Theme) -> Line<'static> {
    let label = card.label();
    let count = format!("{}/{}", card.done_count(), card.steps.len());
    let used = indent.chars().count() + label.chars().count() + 1 + 1 + count.chars().count();
    let mut spans = vec![
        Span::raw(indent.to_string()),
        Span::styled(label, label_style(card, theme)),
        Span::raw(" "),
    ];
    spans.extend(rail(card, width.saturating_sub(used), theme));
    spans.push(Span::styled(format!(" {count}"), theme.muted()));
    Line::from(spans)
}

fn step_line(step: &str, width: usize, theme: &Theme) -> Line<'static> {
    let prefix = format!("  {GLYPH_CURRENT} ");
    let room = width.saturating_sub(prefix.chars().count());
    Line::from(vec![
        Span::styled(prefix, theme.accent()),
        Span::styled(fit(step, room), theme.accent()),
    ])
}

fn doing_line(doing: &str, width: usize, indent: &str, theme: &Theme) -> Line<'static> {
    let prefix = format!("{indent}{GLYPH_PROMPT} ");
    let room = width.saturating_sub(prefix.chars().count());
    Line::from(vec![
        Span::styled(prefix, theme.muted()),
        Span::styled(fit(doing, room), theme.muted()),
    ])
}

pub fn render_minimal(
    f: &mut Frame,
    area: Rect,
    cards: &[&Card],
    feed: &[(Option<String>, &FlowEvent)],
    now: DateTime<Utc>,
    theme: &Theme,
) {
    let width = area.width as usize;
    let height = area.height as usize;
    if cards.is_empty() {
        let line = Line::styled(fit("sara · no active tasks", width), theme.muted());
        f.render_widget(Paragraph::new(line), area);
        return;
    }
    if feed.is_empty() {
        f.render_widget(
            Paragraph::new(rails_with_doing(cards, width, height, theme)),
            area,
        );
        return;
    }
    let budget = (height / 2).max(1);
    let mut lines = rail_block(cards, width, budget, theme, |card, room, lines| {
        if let Some(step) = card.current().map(|c| &card.steps[c].text)
            && room >= 1
        {
            lines.push(step_line(step, width, theme));
        }
    });
    if height >= 10 {
        lines.push(Line::styled("─".repeat(width), theme.muted()));
    }
    let room = height.saturating_sub(lines.len());
    for (label, e) in feed[feed.len().saturating_sub(room)..].iter().rev() {
        lines.push(feed_line(label.as_deref(), e, now, width, theme));
    }
    lines.truncate(height);
    f.render_widget(Paragraph::new(lines), area);
}

fn rails_with_doing(
    cards: &[&Card],
    width: usize,
    height: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    rail_block(cards, width, height, theme, |card, room, lines| {
        if let Some(d) = &card.doing
            && room >= 1
        {
            lines.push(doing_line(d, width, "  ", theme));
        }
    })
}

fn is_grouped(cards: &[&Card]) -> bool {
    cards.iter().any(|c| c.project != cards[0].project)
}

fn project_header(project: &str, width: usize, theme: &Theme) -> Line<'static> {
    let head = format!("── {project} ");
    let pad = width.saturating_sub(head.chars().count());
    Line::styled(
        fit(&format!("{head}{}", "─".repeat(pad)), width),
        theme.muted(),
    )
}

fn rail_block(
    cards: &[&Card],
    width: usize,
    budget: usize,
    theme: &Theme,
    detail: impl Fn(&Card, usize, &mut Vec<Line<'static>>),
) -> Vec<Line<'static>> {
    let grouped = is_grouped(cards);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, card) in cards.iter().enumerate() {
        let more_after = usize::from(i + 1 < cards.len());
        let left = budget.saturating_sub(lines.len());
        let new_group = grouped && (i == 0 || cards[i - 1].project != card.project);
        let header = new_group && left >= 2 + more_after;
        let need = 1 + usize::from(header) + more_after;
        if i > 0 && left < need {
            let more = format!("+{} more", cards.len() - i);
            lines.push(Line::styled(fit(&more, width), theme.muted()));
            break;
        }
        if header {
            lines.push(project_header(&card.project, width, theme));
        }
        lines.push(card_rail_line(card, width, "", theme));
        let room = budget.saturating_sub(lines.len() + more_after);
        let next_header = grouped && i + 1 < cards.len() && cards[i + 1].project != card.project;
        detail(
            card,
            room.saturating_sub(usize::from(next_header)),
            &mut lines,
        );
    }
    lines.truncate(budget.max(1));
    lines
}

fn feed_line(
    label: Option<&str>,
    e: &FlowEvent,
    now: DateTime<Utc>,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let when = format!("{:>3} ", age(now, e.at));
    let glyph = format!("{} ", event_glyph(&e.kind));
    let mut spans = vec![
        Span::styled(when.clone(), theme.muted()),
        Span::styled(glyph.clone(), event_style(&e.kind, theme)),
    ];
    let mut used = when.chars().count() + glyph.chars().count();
    if let Some(l) = label {
        let tag = format!("{l} ");
        used += tag.chars().count();
        spans.push(Span::styled(tag, theme.accent()));
    }
    let text = fit(&event_text(e), width.saturating_sub(used));
    spans.push(Span::styled(text, event_style(&e.kind, theme)));
    Line::from(spans)
}

fn mission_card_lines(
    card: &Card,
    selected: bool,
    now: DateTime<Utc>,
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let marker = if selected {
        Span::styled(format!("{GLYPH_PROMPT} "), theme.accent())
    } else {
        Span::raw("  ")
    };
    let badge = pulse_badge(card.pulse, theme);
    let label = card.label();
    let right = format!("{} · {}", card.project, age(now, card.last));
    let fixed = 2 + badge.width() + 1 + label.chars().count() + 2 + 1 + right.chars().count();
    let desc = fit(&card.description, width.saturating_sub(fixed));
    let gap = width.saturating_sub(fixed + desc.chars().count()) + 1;
    let desc_style = if selected {
        theme.focus()
    } else {
        Style::new()
    };
    let head = Line::from(vec![
        marker,
        badge,
        Span::raw(" "),
        Span::styled(label, label_style(card, theme)),
        Span::raw("  "),
        Span::styled(desc, desc_style),
        Span::raw(" ".repeat(gap)),
        Span::styled(right, theme.muted()),
    ]);

    let mut step = vec![Span::raw("    ")];
    let count = format!(" {}/{}", card.done_count(), card.steps.len());
    let rail_room = (width / 3).max(4);
    let rail = rail(card, rail_room.min(card.steps.len()), theme);
    let rail_w: usize = rail.iter().map(Span::width).sum();
    step.extend(rail);
    step.push(Span::styled(count.clone(), theme.muted()));
    if let Some(cur) = card.current() {
        let room = width.saturating_sub(4 + rail_w + count.chars().count() + 2);
        step.push(Span::raw("  "));
        step.push(Span::raw(fit(&card.steps[cur].text, room)));
    } else if !card.steps.is_empty() {
        step.push(Span::styled("  all steps done", theme.ok()));
    }

    let mut lines = vec![head, Line::from(step)];
    if let Some(d) = &card.doing {
        lines.push(doing_line(d, width, "    ", theme));
    }
    lines
}

pub fn render_mission(f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let c = chrome(area);
    let cards = app.visible_cards(false);
    let live = cards.iter().filter(|c| c.pulse == Pulse::Live).count();
    let stalled = cards.iter().filter(|c| c.pulse == Pulse::Stalled).count();
    let all = app.shows_all_projects();
    let scope = if all {
        "all projects".to_string()
    } else {
        app.project.clone().unwrap_or_default()
    };
    let mut status = vec![Span::styled(format!("{scope} · "), theme.muted())];
    status.push(Span::styled(format!("{live} live"), theme.ok()));
    if stalled > 0 {
        status.push(Span::styled(format!(" · {stalled} stalled"), theme.warn()));
    }
    status.push(Span::raw(" "));
    render_header(f, c.header, theme, "follow · mission control", &status);

    let width = c.body.width as usize;
    if cards.is_empty() {
        let lines = vec![
            Line::raw(""),
            Line::styled("  No agent activity in the last 24h.", theme.muted()),
            Line::styled(
                "  Agents report what they are doing with the `doing` MCP tool.",
                theme.muted(),
            ),
        ];
        f.render_widget(Paragraph::new(lines), c.body);
    } else {
        let blocks: Vec<Vec<Line>> = cards
            .iter()
            .enumerate()
            .map(|(i, card)| mission_card_lines(card, i == app.selected, app.now, width, theme))
            .collect();
        let height = c.body.height as usize;
        let mut start = 0;
        while start < app.selected
            && blocks[start..=app.selected]
                .iter()
                .map(|b| b.len() + 1)
                .sum::<usize>()
                > height
        {
            start += 1;
        }
        let mut lines = Vec::new();
        for b in &blocks[start..] {
            lines.extend(b.iter().cloned());
            lines.push(Line::raw(""));
        }
        f.render_widget(Paragraph::new(lines), c.body);
    }

    let scope_hint = if all { "this project" } else { "all projects" };
    let mut hints = vec![("↑↓", "select"), ("⏎", "follow")];
    if app.project.is_some() {
        hints.push(("p", scope_hint));
    }
    hints.extend([("m", "minimal"), ("q", "quit")]);
    render_footer(f, c.footer, theme, &hints);
}

fn event_glyph(kind: &FlowKind) -> &'static str {
    match kind {
        FlowKind::Doing => GLYPH_PROMPT,
        FlowKind::StepDone => GLYPH_DONE,
        FlowKind::StepAdded => GLYPH_OPEN,
        FlowKind::Note(_) => "»",
        FlowKind::Memory(_) => "≈",
        FlowKind::Change(_) => "·",
        FlowKind::AiRun => "λ",
    }
}

fn event_text(e: &FlowEvent) -> String {
    match &e.kind {
        FlowKind::StepDone => match &e.detail {
            Some(d) if !d.is_empty() => format!("{} — {d}", e.text),
            _ => e.text.clone(),
        },
        FlowKind::StepAdded => format!("step added: {}", e.text),
        FlowKind::Note(k) => format!("{k}: {}", e.text),
        FlowKind::Memory(k) => format!("{k} {}", e.text),
        FlowKind::AiRun => format!("ai run: {}", e.text),
        FlowKind::Doing | FlowKind::Change(_) => e.text.clone(),
    }
}

fn event_style(kind: &FlowKind, theme: &Theme) -> Style {
    match kind {
        FlowKind::Doing => theme.accent(),
        FlowKind::StepDone => theme.ok(),
        FlowKind::Note(_) | FlowKind::Memory(_) => Style::new(),
        FlowKind::StepAdded | FlowKind::Change(_) | FlowKind::AiRun => theme.muted(),
    }
}

pub fn render_task(
    f: &mut Frame,
    area: Rect,
    app: &App,
    card: &Card,
    events: &[FlowEvent],
    theme: &Theme,
) {
    let c = chrome(area);
    let status = vec![
        pulse_badge(card.pulse, theme),
        Span::styled(format!(" {} ", age(app.now, card.last)), theme.muted()),
    ];
    render_header(
        f,
        c.header,
        theme,
        &format!("follow · {}", card.label()),
        &status,
    );

    let width = c.body.width as usize;
    let mut lines = vec![
        Line::styled(
            fit(&format!(" {}", card.description), width),
            theme.accent(),
        ),
        card_rail_line(card, width, " ", theme),
    ];
    match card.current() {
        Some(i) => lines.push(Line::from(vec![
            Span::styled(format!(" {GLYPH_CURRENT} "), theme.accent()),
            Span::raw(fit(&card.steps[i].text, width.saturating_sub(3))),
        ])),
        None if !card.steps.is_empty() => {
            lines.push(Line::styled(
                format!(" {GLYPH_DONE} all steps done"),
                theme.ok(),
            ));
        }
        None => {}
    }
    if let Some(d) = &card.doing {
        lines.push(doing_line(d, width, " ", theme));
    }
    lines.push(Line::styled(
        fit(&format!(" ── flow {}", "─".repeat(width)), width),
        theme.muted(),
    ));

    let room = (c.body.height as usize).saturating_sub(lines.len());
    let tail = &events[events.len().saturating_sub(room)..];
    for e in tail {
        let when = format!(" {:>3} ", age(app.now, e.at));
        let glyph = event_glyph(&e.kind);
        let text = fit(
            &event_text(e),
            width.saturating_sub(when.chars().count() + 2),
        );
        lines.push(Line::from(vec![
            Span::styled(when, theme.muted()),
            Span::styled(format!("{glyph} "), event_style(&e.kind, theme)),
            Span::styled(text, event_style(&e.kind, theme)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), c.body);

    let back = if app.opened_from_mission {
        "back"
    } else {
        "quit"
    };
    render_footer(
        f,
        c.footer,
        theme,
        &[("esc", back), ("m", "minimal"), ("q", "quit")],
    );
}
