use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::commands::shared::strength_label;
use crate::infrastructure::tui;

use crate::commands::dream::layout::{MIN_EDGE, bond_exists};
use crate::commands::dream::types::WebData;

#[allow(clippy::too_many_arguments)]
pub(in crate::commands::dream) fn ui_web(
    f: &mut ratatui::Frame,
    web: &WebData,
    frame: u64,
    selected: usize,
    show_help: bool,
    zoom: f64,
    search_input: Option<&str>,
    query: &str,
) {
    let area = f.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

    let strong = web.stars.iter().filter(|s| s.strength >= 2.0).count();
    let linked = web
        .stars
        .iter()
        .filter(|s| s.strength >= 1.5 && s.strength < 2.0)
        .count();
    let weak = web.stars.len() - strong - linked;
    let matches = web.stars.iter().filter(|s| s.matches(query)).count();
    let mut header = vec![
        Span::styled(
            " ✦ the web ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "— {} memories · {} bonds · ",
                web.stars.len(),
                web.bonds.len()
            ),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("{strong} strong "),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            format!("{linked} linked "),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(format!("{weak} weak"), Style::default().fg(Color::DarkGray)),
    ];
    if let Some(buf) = search_input {
        header.push(Span::styled(
            format!("   /{buf}▌"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    } else if !query.is_empty() {
        header.push(Span::styled(
            format!("   /{query} — {matches} lit (n next, Esc clear)"),
            Style::default().fg(Color::Yellow),
        ));
    }
    if zoom > 1.0 {
        header.push(Span::styled(
            format!("   {zoom:.1}x"),
            Style::default().fg(Color::Green),
        ));
    }
    header.push(Span::styled(
        "   [?] help  [q] wake up",
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(Paragraph::new(Line::from(header)), outer[0]);

    let sel_star = &web.stars[selected];
    let (cx, cy) = (
        sel_star.x * (1.0 - 1.0 / zoom),
        sel_star.y * (1.0 - 1.0 / zoom),
    );
    let tx = move |x: f64| (x - cx) * zoom;
    let ty = move |y: f64| (y - cy) * zoom;

    let searching = !query.is_empty();
    let canvas = Canvas::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .x_bounds([-100.0, 100.0])
        .y_bounds([-75.0, 75.0])
        .paint(move |ctx| {
            for l in &web.links {
                if bond_exists(&web.bonds, l.a, l.b) {
                    continue;
                }
                let (sa, sb) = (&web.stars[l.a], &web.stars[l.b]);
                let touches_sel = l.a == selected || l.b == selected;
                let faded = searching && !(sa.matches(query) || sb.matches(query));
                let t = ((l.weight - MIN_EDGE) / (1.0 - MIN_EDGE)).clamp(0.0, 1.0);
                let color = if faded {
                    Color::Rgb(18, 19, 23)
                } else if touches_sel {
                    let v = (60.0 + 90.0 * t) as u8;
                    Color::Rgb(30, v, v)
                } else {
                    let v = (26.0 + 34.0 * t) as u8;
                    Color::Rgb(v, v, (v as u16 + 8) as u8)
                };
                ctx.draw(&CanvasLine {
                    x1: tx(sa.x),
                    y1: ty(sa.y),
                    x2: tx(sb.x),
                    y2: ty(sb.y),
                    color,
                });
            }
            for b in &web.bonds {
                let (sa, sb) = (&web.stars[b.a], &web.stars[b.b]);
                let touches_sel = b.a == selected || b.b == selected;
                let faded = searching && !(sa.matches(query) || sb.matches(query));
                let color = match (b.relation.as_str(), touches_sel) {
                    _ if faded => Color::Rgb(30, 32, 38),
                    ("supersedes", true) => Color::Red,
                    ("supersedes", false) => Color::Rgb(110, 40, 40),
                    (_, true) => Color::Cyan,
                    (_, false) => Color::Rgb(50, 70, 80),
                };
                ctx.draw(&CanvasLine {
                    x1: tx(sa.x),
                    y1: ty(sa.y),
                    x2: tx(sb.x),
                    y2: ty(sb.y),
                    color,
                });
            }
            let breath = ((frame as f64 * 0.06).sin() + 1.0) / 2.0;
            for (i, s) in web.stars.iter().enumerate() {
                let is_sel = i == selected;
                let is_match = s.matches(query);
                let pulse = if s.recently_recalled { breath } else { 0.35 };
                let lum =
                    (70.0 + 150.0 * (s.strength / 2.5).min(1.0) * (0.55 + 0.45 * pulse)) as u8;
                let (glyph, color) = if s.provisional {
                    ("◌", Color::Rgb(lum, 50, lum))
                } else if s.strength >= 2.0 {
                    ("✦", Color::Rgb(lum, lum, lum))
                } else if s.strength >= 1.5 {
                    ("✧", Color::Rgb(50, lum, lum))
                } else {
                    (
                        "·",
                        Color::Rgb(lum / 2, lum / 2, (lum as u16 + 30).min(255) as u8),
                    )
                };
                let style = if is_sel {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if searching && is_match {
                    Style::default()
                        .fg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD)
                } else if searching {
                    Style::default().fg(Color::Rgb(45, 45, 52))
                } else {
                    Style::default().fg(color)
                };
                let text = if is_sel {
                    format!("{glyph} ◄")
                } else {
                    glyph.to_string()
                };
                ctx.print(tx(s.x), ty(s.y), Line::from(Span::styled(text, style)));
            }
        });
    f.render_widget(canvas, outer[1]);

    let s = &web.stars[selected];
    let bonds_of: Vec<String> = web
        .bonds
        .iter()
        .filter_map(|b| {
            if b.a == selected {
                Some(format!("{} {}", b.relation, web.stars[b.b].label))
            } else if b.b == selected {
                Some(format!("⟵ {} {}", b.relation, web.stars[b.a].label))
            } else {
                None
            }
        })
        .collect();
    let mut foot = vec![Line::from(vec![
        Span::styled(
            format!("{} ", s.label),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "{} ({:.1}){} ",
                strength_label(s.strength),
                s.strength,
                if s.provisional { " [provisional]" } else { "" }
            ),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            format!("[{}]  ", s.tags.join(", ")),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(s.title.clone(), Style::default().fg(Color::Gray)),
    ])];
    foot.push(Line::from(Span::styled(
        if bonds_of.is_empty() {
            "no bonds — an isolated thought (Enter to dream into it)".to_string()
        } else {
            format!("bonds: {}  (Enter to dream into it)", bonds_of.join(" · "))
        },
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(
        Paragraph::new(foot).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        outer[2],
    );

    if show_help {
        tui::render_help_overlay(
            f,
            "the web",
            &[
                ("arrows", "move to the nearest star that way"),
                ("Tab / ⇧Tab", "cycle stars in order"),
                ("/", "search (label, tags, title, body)"),
                ("n / N", "next / previous match"),
                ("+ / - / 0", "zoom toward the selected star / reset"),
                ("Enter", "dream into the selected memory"),
                ("Esc", "zoom out of a dream / clear search / wake up"),
                ("q", "wake up"),
            ],
        );
    }
}
