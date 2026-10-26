use chrono::{DateTime, Duration, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::infrastructure::db::{FlowEvent, FlowKind};
use crate::infrastructure::tui::keymap::help;
use crate::infrastructure::tui::render_help_overlay;
use crate::infrastructure::tui::screen::{chrome, render_footer, render_header};
use crate::infrastructure::tui::theme::{
    GLYPH_CURRENT, GLYPH_DONE, GLYPH_OPEN, GLYPH_PROMPT, Theme, Tone,
};

use super::state::{App, Card, Mode, Pulse, in_feed, is_fresh, is_minimal, is_split};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPARK: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
const SPARK_MIN_WIDTH: usize = 60;

type FeedRow<'a> = (Option<String>, &'a FlowEvent);

pub fn render(f: &mut Frame, app: &App, theme: &Theme) {
    let area = f.area();
    let minimal = is_minimal(app.minimal, area.width, area.height);
    match (app.mode, &app.focus) {
        (Mode::Task(_), Some(focus)) if minimal => {
            let feed: Vec<FeedRow> = focus
                .events
                .iter()
                .filter(|e| in_feed(e))
                .map(|e| (None, e))
                .collect();
            render_minimal(f, area, app, &[&focus.card], &feed, theme)
        }
        (Mode::Task(_), Some(focus)) => {
            render_task(f, area, app, &focus.card, &focus.events, theme)
        }
        (Mode::Task(_), None) => {}
        (Mode::Mission, _) if minimal => {
            let cards = app.visible_cards(true);
            let feed = feed_rows(app, &cards, true);
            render_minimal(f, area, app, &cards, &feed, theme)
        }
        (Mode::Mission, _) => render_mission(f, area, app, theme),
    }
    if app.help {
        let title = match app.mode {
            Mode::Mission => "follow · mission control",
            Mode::Task(_) => "follow · task",
        };
        render_help_overlay(f, title, &help_bindings(app));
    }
}

fn help_bindings(app: &App) -> Vec<(&'static str, &'static str)> {
    let mut b = match app.mode {
        Mode::Mission => vec![
            help::MOVE,
            help::TOP_BOTTOM,
            ("Enter", "follow the selected task"),
            ("o", "open the selected task in sara info"),
            ("/", "filter by text (Esc clears)"),
            ("i", "hide / show idle tasks"),
            ("p", "all projects / this project"),
        ],
        Mode::Task(_) => vec![
            ("j/k, ↓/↑", "scroll the flow"),
            help::PAGE,
            ("gg / G", "oldest / follow the latest"),
            ("o", "open the task in sara info"),
            ("Esc", "back to mission control"),
        ],
    };
    b.extend([
        ("m", "toggle minimal mode"),
        ("Space", "pause / resume live updates"),
        ("q", "quit"),
        help::HELP,
    ]);
    b
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

/// A compact duration: `45s`, `12m`, `1h05m`, `3d`.
pub fn span(d: Duration) -> String {
    let s = d.num_seconds().max(0);
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h{:02}m", s / 3600, s % 3600 / 60),
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

fn width_of(spans: &[Span]) -> usize {
    spans.iter().map(Span::width).sum()
}

fn pulse_tone(p: Pulse) -> Tone {
    match p {
        Pulse::Live => Tone::Ok,
        Pulse::Stalled => Tone::Warn,
        Pulse::Idle => Tone::Muted,
    }
}

fn pulse_badge(p: Pulse, frame: usize, theme: &Theme) -> Span<'static> {
    let text = match p {
        Pulse::Live => format!(" {} LIVE ", SPINNER[frame % SPINNER.len()]),
        Pulse::Stalled => " STALLED ".to_string(),
        Pulse::Idle => " IDLE ".to_string(),
    };
    Span::styled(text, theme.badge(pulse_tone(p)))
}

fn paused_badge(app: &App, theme: &Theme) -> Option<Span<'static>> {
    app.paused
        .then(|| Span::styled(" PAUSED ", theme.badge(Tone::Warn)))
}

pub fn sparkline(buckets: &[u32], theme: &Theme) -> Vec<Span<'static>> {
    let max = buckets.iter().copied().max().unwrap_or(0) as usize;
    buckets
        .iter()
        .map(|&c| match c as usize {
            0 => Span::styled(SPARK[0], theme.muted()),
            c => Span::styled(SPARK[(1 + (c * 7 - 1) / max).min(7)], theme.accent()),
        })
        .collect()
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

fn filter_prompt(app: &App, width: usize, theme: &Theme) -> Line<'static> {
    let hint = "  ⏎ keep · esc clear";
    let room = width.saturating_sub(2 + hint.chars().count());
    let shown: String = {
        let n = app.filter.chars().count();
        app.filter.chars().skip(n.saturating_sub(room)).collect()
    };
    Line::from(vec![
        Span::styled("/", theme.accent()),
        Span::raw(shown),
        Span::styled("▏", theme.accent()),
        Span::styled(hint, theme.muted()),
    ])
}

fn feed_rows<'a>(app: &'a App, cards: &[&Card], minimal: bool) -> Vec<FeedRow<'a>> {
    let many = cards.len() > 1;
    let grouped = is_grouped(cards);
    app.visible_feed(minimal)
        .into_iter()
        .map(|i| {
            let label = if grouped {
                Some(format!("{} {}", i.project, i.label))
            } else {
                many.then(|| i.label.clone())
            };
            (label, &i.event)
        })
        .collect()
}

pub fn render_minimal(
    f: &mut Frame,
    area: Rect,
    app: &App,
    cards: &[&Card],
    feed: &[FeedRow],
    theme: &Theme,
) {
    let width = area.width as usize;
    let mut height = area.height as usize;
    let prompt =
        (app.filtering || !app.filter.is_empty()).then(|| filter_prompt(app, width, theme));
    if prompt.is_some() && height > 1 {
        height -= 1;
    }
    let mut lines = minimal_lines(
        cards,
        feed,
        app.now,
        width,
        height,
        app.is_narrowed(),
        theme,
    );
    if let Some(p) = prompt {
        lines.resize(height, Line::raw(""));
        lines.push(p);
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn minimal_lines(
    cards: &[&Card],
    feed: &[FeedRow],
    now: DateTime<Utc>,
    width: usize,
    height: usize,
    narrowed: bool,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if cards.is_empty() {
        let text = if narrowed {
            "sara · no tasks match"
        } else {
            "sara · no active tasks"
        };
        return vec![Line::styled(fit(text, width), theme.muted())];
    }
    if feed.is_empty() {
        return rails_with_doing(cards, width, height, theme);
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
    lines
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

fn age_span(now: DateTime<Utc>, at: DateTime<Utc>, pad: &str, theme: &Theme) -> Span<'static> {
    let style = if is_fresh(at, now) {
        theme.accent()
    } else {
        theme.muted()
    };
    Span::styled(format!("{pad}{:>3} ", age(now, at)), style)
}

fn feed_line(
    label: Option<&str>,
    e: &FlowEvent,
    now: DateTime<Utc>,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let when = age_span(now, e.at, "", theme);
    let glyph = format!("{} ", event_glyph(&e.kind));
    let mut used = when.width() + glyph.chars().count();
    let mut spans = vec![when, Span::styled(glyph, event_style(&e.kind, theme))];
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
    app: &App,
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let marker = if selected {
        Span::styled(format!("{GLYPH_PROMPT} "), theme.accent())
    } else {
        Span::raw("  ")
    };
    let badge = pulse_badge(card.pulse, app.frame, theme);
    let label = card.label();
    let right = format!("{} · {}", card.project, age(app.now, card.last));
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

    let spark = if width >= SPARK_MIN_WIDTH {
        sparkline(&card.spark, theme)
    } else {
        Vec::new()
    };
    let spark_w = if spark.is_empty() {
        0
    } else {
        width_of(&spark) + 2
    };
    let mut step = vec![Span::raw("    ")];
    let count = format!(" {}/{}", card.done_count(), card.steps.len());
    let rail_room = (width / 3).max(4);
    step.extend(rail(card, rail_room.min(card.steps.len()), theme));
    step.push(Span::styled(count, theme.muted()));
    let used = width_of(&step);
    if let Some(cur) = card.current() {
        let room = width.saturating_sub(used + 2 + spark_w);
        step.push(Span::raw("  "));
        step.push(Span::raw(fit(&card.steps[cur].text, room)));
    } else if !card.steps.is_empty() {
        step.push(Span::styled("  all steps done", theme.ok()));
    }
    if !spark.is_empty() {
        let pad = width.saturating_sub(width_of(&step) + width_of(&spark));
        step.push(Span::raw(" ".repeat(pad)));
        step.extend(spark);
    }

    let mut lines = vec![head, Line::from(step)];
    if let Some(d) = &card.doing {
        lines.push(doing_line(d, width, "    ", theme));
    }
    lines
}

fn mission_status(app: &App, cards: &[&Card], room: usize, theme: &Theme) -> Vec<Span<'static>> {
    let live = cards.iter().filter(|c| c.pulse == Pulse::Live).count();
    let stalled = cards.iter().filter(|c| c.pulse == Pulse::Stalled).count();
    let scope = if app.shows_all_projects() {
        "all projects".to_string()
    } else {
        app.project.clone().unwrap_or_default()
    };
    let paused = paused_badge(app, theme).map(|b| vec![b, Span::raw(" ")]);
    let filter = (!app.filter.is_empty() && !app.filtering)
        .then(|| vec![Span::styled(format!("/{} · ", app.filter), theme.accent())]);
    let idle = app
        .hide_idle
        .then(|| vec![Span::styled("idle hidden · ", theme.muted())]);
    let scope = vec![Span::styled(format!("{scope} · "), theme.muted())];
    let mut counts = vec![Span::styled(format!("{live} live"), theme.ok())];
    if stalled > 0 {
        counts.push(Span::styled(format!(" · {stalled} stalled"), theme.warn()));
    }
    counts.push(Span::raw(" "));

    // Shed the least important parts until the status fits beside the title.
    let mut parts: Vec<Option<Vec<Span<'static>>>> =
        vec![paused, filter, idle, Some(scope), Some(counts)];
    for drop in [3, 2, 1] {
        let w: usize = parts.iter().flatten().map(|p| width_of(p)).sum();
        if w <= room {
            break;
        }
        parts[drop] = None;
    }
    parts.into_iter().flatten().flatten().collect()
}

pub fn render_mission(f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let c = chrome(area);
    let cards = app.visible_cards(false);
    let title = "follow · mission control";
    let room = (area.width as usize).saturating_sub(title.chars().count() + 10);
    render_header(
        f,
        c.header,
        theme,
        title,
        &mission_status(app, &cards, room, theme),
    );

    let (list, side) = if is_split(area.width) {
        let [l, r] = Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
            .areas(c.body);
        (
            Rect {
                width: l.width.saturating_sub(1),
                ..l
            },
            Some(r),
        )
    } else {
        (c.body, None)
    };
    render_cards(f, list, app, &cards, theme);
    if let Some(side) = side {
        render_side_feed(f, side, app, &cards, theme);
    }

    if app.filtering {
        f.render_widget(
            Paragraph::new(filter_prompt(app, c.footer.width as usize, theme)),
            c.footer,
        );
        return;
    }
    let idle = if app.hide_idle {
        "show idle"
    } else {
        "hide idle"
    };
    render_footer(
        f,
        c.footer,
        theme,
        &[
            ("↑↓", "select"),
            ("⏎", "follow"),
            ("o", "info"),
            ("/", "filter"),
            ("i", idle),
            ("?", "help"),
            ("q", "quit"),
        ],
    );
}

fn render_cards(f: &mut Frame, area: Rect, app: &App, cards: &[&Card], theme: &Theme) {
    let width = area.width as usize;
    if cards.is_empty() {
        let lines = if app.cards.is_empty() {
            vec![
                Line::raw(""),
                Line::styled("  No agent activity in the last 24h.", theme.muted()),
                Line::styled(
                    "  Agents report what they are doing with the `doing` MCP tool.",
                    theme.muted(),
                ),
            ]
        } else {
            vec![
                Line::raw(""),
                Line::styled("  No tasks match.", theme.muted()),
                Line::styled(
                    "  Esc clears the filter, i shows idle tasks, p widens the scope.",
                    theme.muted(),
                ),
            ]
        };
        f.render_widget(Paragraph::new(lines), area);
        return;
    }
    let blocks: Vec<Vec<Line>> = cards
        .iter()
        .enumerate()
        .map(|(i, card)| mission_card_lines(card, i == app.selected, app, width, theme))
        .collect();
    let height = area.height as usize;
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
    f.render_widget(Paragraph::new(lines), area);
}

fn render_side_feed(f: &mut Frame, area: Rect, app: &App, cards: &[&Card], theme: &Theme) {
    let block = Block::new()
        .borders(Borders::LEFT)
        .border_style(theme.muted())
        .title(Span::styled(" live feed ", theme.muted()));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let feed = feed_rows(app, cards, false);
    let width = inner.width as usize;
    let lines: Vec<Line> = if feed.is_empty() {
        vec![Line::styled(" nothing yet", theme.muted())]
    } else {
        feed.iter()
            .rev()
            .take(inner.height as usize)
            .map(|(label, e)| feed_line(label.as_deref(), e, app.now, width, theme))
            .collect()
    };
    f.render_widget(Paragraph::new(lines), inner);
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

fn rule(title: &str, width: usize, theme: &Theme) -> Line<'static> {
    let head = format!(" ── {title} ");
    let pad = width.saturating_sub(head.chars().count());
    Line::styled(
        fit(&format!("{head}{}", "─".repeat(pad)), width),
        theme.muted(),
    )
}

fn task_meta_line(card: &Card, now: DateTime<Utc>, width: usize, theme: &Theme) -> Line<'static> {
    let mut tail = vec![Span::styled(
        format!("  {} · {} elapsed", card.project, span(now - card.entry)),
        theme.muted(),
    )];
    if width >= SPARK_MIN_WIDTH && !card.spark.is_empty() {
        tail.push(Span::raw("  "));
        tail.extend(sparkline(&card.spark, theme));
    }
    let tail_w = width_of(&tail);
    let Line { mut spans, .. } = card_rail_line(card, width.saturating_sub(tail_w), " ", theme);
    if width_of(&spans) + tail_w <= width {
        spans.extend(tail);
    }
    Line::from(spans)
}

fn plan_line(
    card: &Card,
    i: usize,
    took: Option<Duration>,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let step = &card.steps[i];
    let current = card.current() == Some(i);
    let (glyph, glyph_style, text_style) = match (step.done, current) {
        (true, _) => (GLYPH_DONE, theme.ok(), Style::new()),
        (false, true) => (GLYPH_CURRENT, theme.accent(), theme.accent()),
        (false, false) => (GLYPH_OPEN, theme.muted(), theme.muted()),
    };
    let right = took.map(span).unwrap_or_default();
    let right_style = if current {
        theme.accent()
    } else {
        theme.muted()
    };
    let lead = format!(" {glyph} ");
    let room = width.saturating_sub(lead.chars().count() + right.chars().count() + 1);
    let text = fit(&step.text, room);
    let mut spans = vec![
        Span::styled(lead, glyph_style),
        Span::styled(text.clone(), text_style),
    ];
    let mut used = text.chars().count();
    if let Some(result) = step.result.as_deref()
        && room > used + 4
    {
        let r = fit(&format!(" — {result}"), room - used);
        used += r.chars().count();
        spans.push(Span::styled(r, theme.muted()));
    }
    spans.push(Span::raw(" ".repeat(room - used + 1)));
    spans.push(Span::styled(right, right_style));
    Line::from(spans)
}

fn plan_lines(
    card: &Card,
    now: DateTime<Utc>,
    budget: usize,
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let n = card.steps.len();
    let budget = budget.max(3).min(n);
    if budget == 0 {
        return Vec::new();
    }
    let times = card.step_times(now);
    let cur = card.current().unwrap_or(n - 1);
    let start = cur.saturating_sub(budget / 3).min(n - budget);
    let end = start + budget;
    let from = start + usize::from(start > 0);
    let to = end - usize::from(end < n);
    let mut lines = Vec::new();
    if from > start {
        lines.push(Line::styled(format!("   ⋮ {from} earlier"), theme.muted()));
    }
    for (i, took) in times.iter().enumerate().take(to).skip(from) {
        lines.push(plan_line(card, i, *took, width, theme));
    }
    if to < end {
        lines.push(Line::styled(format!("   ⋮ {} more", n - to), theme.muted()));
    }
    lines
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
    let mut status = Vec::new();
    if let Some(b) = paused_badge(app, theme) {
        status.extend([b, Span::raw(" ")]);
    }
    status.push(pulse_badge(card.pulse, app.frame, theme));
    status.push(Span::styled(
        format!(" {} ", age(app.now, card.last)),
        theme.muted(),
    ));
    render_header(
        f,
        c.header,
        theme,
        &format!("follow · {}", card.label()),
        &status,
    );

    let width = c.body.width as usize;
    let height = c.body.height as usize;
    let mut lines = vec![
        Line::styled(
            fit(&format!(" {}", card.description), width),
            theme.accent(),
        ),
        task_meta_line(card, app.now, width, theme),
    ];
    if !card.steps.is_empty() {
        lines.push(rule("plan", width, theme));
        let budget = height.saturating_sub(6) / 2;
        lines.extend(plan_lines(card, app.now, budget, width, theme));
    }
    if let Some(d) = &card.doing {
        lines.push(doing_line(d, width, " ", theme));
    }

    let room = height.saturating_sub(lines.len() + 1);
    app.flow_room.set(Some(room));
    let scroll = app.scroll.min(events.len().saturating_sub(room.max(1)));
    let flow = if scroll > 0 {
        format!("flow · ↓ {scroll} newer")
    } else {
        "flow".to_string()
    };
    lines.push(rule(&flow, width, theme));

    let end = events.len() - scroll;
    for e in &events[end.saturating_sub(room)..end] {
        let when = age_span(app.now, e.at, " ", theme);
        let glyph = event_glyph(&e.kind);
        let text = fit(&event_text(e), width.saturating_sub(when.width() + 2));
        lines.push(Line::from(vec![
            when,
            Span::styled(format!("{glyph} "), event_style(&e.kind, theme)),
            Span::styled(text, event_style(&e.kind, theme)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), c.body);

    let mut hints = Vec::new();
    if app.opened_from_mission {
        hints.push(("esc", "back"));
    }
    hints.extend([
        ("↑↓", "scroll"),
        ("G", "latest"),
        ("o", "info"),
        ("?", "help"),
        ("q", "quit"),
    ]);
    render_footer(f, c.footer, theme, &hints);
}
