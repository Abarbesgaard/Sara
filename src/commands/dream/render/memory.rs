use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Sparkline, Wrap};

use crate::commands::shared::strength_label;
use crate::infrastructure::tui;

use super::effects::{PULSE_TICKS, accent, body_style, materialized_body};
use crate::commands::dream::types::{DreamData, NodeKind};

pub(in crate::commands::dream) fn ui(
    f: &mut ratatui::Frame,
    data: &DreamData,
    frame: u64,
    selected: usize,
    breadcrumb: &[String],
    show_help: bool,
) {
    let area = f.area();
    let ac = accent(data.provisional);

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let mut path: Vec<Span> = vec![Span::styled(" ✦ ", Style::default().fg(ac))];
    for b in breadcrumb {
        path.push(Span::styled(
            b.clone(),
            Style::default().fg(Color::DarkGray),
        ));
        path.push(Span::styled(" ⟶ ", Style::default().fg(Color::DarkGray)));
    }
    path.push(Span::styled(
        data.label.clone(),
        Style::default().fg(ac).add_modifier(Modifier::BOLD),
    ));
    if data.provisional {
        path.push(Span::styled(
            "  (provisional — an unreviewed dream)",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::ITALIC),
        ));
    }
    path.push(Span::styled(
        "   [?] help  [q] wake up",
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(Paragraph::new(Line::from(path)), outer[0]);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(outer[1]);

    render_neuron(f, cols[0], data, frame, selected);
    render_detail(f, cols[1], data, frame, selected);

    if show_help {
        tui::render_help_overlay(
            f,
            "dreaming",
            &[
                ("Tab / arrows", "select a dendrite"),
                ("Enter", "drift into the linked memory"),
                ("Backspace / Esc", "drift back along your path"),
                ("q", "wake up"),
            ],
        );
    }
}

fn render_neuron(
    f: &mut ratatui::Frame,
    area: Rect,
    data: &DreamData,
    frame: u64,
    selected: usize,
) {
    let ac = accent(data.provisional);
    let n = data.neighbors.len();

    let canvas = Canvas::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if data.provisional {
                    Color::Magenta
                } else {
                    Color::DarkGray
                }))
                .title(Span::styled(" the neuron ", Style::default().fg(ac))),
        )
        .x_bounds([-100.0, 100.0])
        .y_bounds([-75.0, 75.0])
        .paint(move |ctx| {
            if frame < PULSE_TICKS {
                let t = frame as f64 / PULSE_TICKS as f64;
                for lag in 0..3 {
                    let r = (t - lag as f64 * 0.18).max(0.0) * 70.0;
                    if r > 0.5 {
                        ctx.draw(&Circle {
                            x: 0.0,
                            y: 0.0,
                            radius: r,
                            color: Color::Rgb(
                                (80.0 * (1.0 - t)) as u8 + 20,
                                (200.0 * (1.0 - t)) as u8 + 30,
                                (220.0 * (1.0 - t)) as u8 + 35,
                            ),
                        });
                    }
                }
                let float_y = 8.0 + t * 30.0;
                ctx.print(
                    6.0,
                    float_y,
                    Line::from(Span::styled(
                        "+0.1",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )),
                );
            }

            for (i, nb) in data.neighbors.iter().enumerate() {
                let angle = std::f64::consts::TAU * (i as f64 / n.max(1) as f64)
                    + 0.35
                    + (frame as f64 * 0.004);
                let (r_x, r_y) = (72.0, 52.0);
                let x = angle.cos() * r_x;
                let y = angle.sin() * r_y;
                let is_sel = i == selected;

                let (edge_color, glyph, name) = match &nb.kind {
                    NodeKind::Memory { label, relation } => {
                        let c = if relation.contains("supersedes") {
                            Color::Red
                        } else {
                            Color::Cyan
                        };
                        (c, "●", format!("{label} {relation}"))
                    }
                    NodeKind::Task { id, source, .. } => {
                        (Color::Yellow, "◆", format!("#{id} ({source})"))
                    }
                    NodeKind::File { name } => (Color::Green, "▪", name.clone()),
                };
                let dim = Color::Rgb(60, 60, 70);
                ctx.draw(&CanvasLine {
                    x1: 0.0,
                    y1: 0.0,
                    x2: x,
                    y2: y,
                    color: if is_sel { edge_color } else { dim },
                });
                let node_style = if is_sel {
                    Style::default().fg(edge_color).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(dim)
                };
                ctx.print(
                    x,
                    y,
                    Line::from(Span::styled(format!("{glyph} {name}"), node_style)),
                );
            }

            let breath = ((frame as f64 * 0.06).sin() + 1.0) / 2.0;
            let glyphs = ["◌", "○", "◎", "◉"];
            let gi = (breath * (glyphs.len() - 1) as f64).round() as usize;
            let lum = (120.0 + breath * 120.0 * (data.strength / 2.5).min(1.0)) as u8;
            let core_color = if data.provisional {
                Color::Rgb(lum, 60, lum)
            } else {
                Color::Rgb(60, lum, lum)
            };
            ctx.draw(&Circle {
                x: 0.0,
                y: 0.0,
                radius: 4.0 + breath * 2.0,
                color: core_color,
            });
            ctx.print(
                -3.0,
                0.0,
                Line::from(Span::styled(
                    format!("{} {}", glyphs[gi], data.label),
                    Style::default().fg(core_color).add_modifier(Modifier::BOLD),
                )),
            );
        });
    f.render_widget(canvas, area);
}

fn render_detail(
    f: &mut ratatui::Frame,
    area: Rect,
    data: &DreamData,
    frame: u64,
    selected: usize,
) {
    let ac = accent(data.provisional);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(4),
        ])
        .split(area);

    let ratio = (data.strength / 2.5).min(1.0);
    let gauge_color = if data.strength >= 2.0 {
        Color::White
    } else if data.strength >= 1.5 {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    f.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(Span::styled(" vividness ", Style::default().fg(ac))),
            )
            .gauge_style(Style::default().fg(gauge_color).bg(Color::Rgb(25, 25, 32)))
            .ratio(ratio)
            .label(format!(
                "{} ({:.1})",
                strength_label(data.strength),
                data.strength
            )),
        rows[0],
    );

    f.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(Span::styled(
                        format!(" recall pulse — {} in 30d ", data.recall_total_30d),
                        Style::default().fg(ac),
                    )),
            )
            .data(&data.sparkline)
            .style(Style::default().fg(Color::Cyan)),
        rows[1],
    );

    let resolved = materialized_body(&data.item.body, frame, data.strength);
    let real_style = body_style(data.strength, data.provisional);
    let noise_style = Style::default().fg(Color::Rgb(60, 60, 80));
    let mut lines: Vec<Line> = vec![];
    let mut spans: Vec<Span> = vec![];
    for (c, is_real) in resolved {
        if c == '\n' {
            lines.push(Line::from(std::mem::take(&mut spans)));
        } else {
            spans.push(Span::styled(
                c.to_string(),
                if is_real { real_style } else { noise_style },
            ));
        }
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    let age = chrono::Utc::now()
        .signed_duration_since(data.item.created)
        .num_days();
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(
                    format!(" the memory — {age}d old "),
                    Style::default().fg(ac),
                )),
        ),
        rows[2],
    );

    let mut foot: Vec<Line> = vec![Line::from(vec![
        Span::styled("tags ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            data.item.tags.join(", "),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(
            format!("   files {}", data.files.len()),
            Style::default().fg(Color::DarkGray),
        ),
    ])];
    if let Some(nb) = data.neighbors.get(selected) {
        let desc = match &nb.kind {
            NodeKind::Memory { label, relation } => {
                format!("● {label} — {relation} (Enter to drift into it)")
            }
            NodeKind::Task { id, desc, source } => format!("◆ task #{id} ({source}) — {desc}"),
            NodeKind::File { name } => format!("▪ {name}"),
        };
        foot.push(Line::from(Span::styled(
            desc,
            Style::default().fg(Color::Gray),
        )));
    }
    f.render_widget(
        Paragraph::new(foot).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        rows[3],
    );
}
