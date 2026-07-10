//! `sara dream <label>` — immersive TUI for peeking into a single memory.
//!
//! The memory is rendered as a neuron: a breathing cell body at the centre of
//! a canvas, with dendrites radiating out to linked memories, tasks and files.
//! On entry the body text materialises out of noise (weak memories take longer
//! to resolve), a recall pulse ripples outward, and — because peeking at a
//! memory *is* a recall — a `memory_recalled` event is recorded and a `+0.1`
//! floats up from the cell body. Frequently-viewed memories literally stay
//! alive: the observer effect as a feature.
//!
//! Non-TTY stdout falls back to a plain single-memory digest.

use anyhow::Result;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Sparkline, Wrap};
use rusqlite::Connection;

use crate::infrastructure::db;
use crate::infrastructure::model::Item;
use crate::infrastructure::tui;

const TICK_MS: u64 = 50;
const PULSE_TICKS: u64 = 40;

/// What a dendrite points at.
#[derive(Clone)]
enum NodeKind {
    /// Another memory (enterable): label + relation.
    Memory { label: String, relation: String },
    /// A linked task: display id + description + link source (auto/explicit).
    Task { id: String, desc: String, source: String },
    /// An associated file path.
    File { name: String },
}

#[derive(Clone)]
struct Neighbor {
    kind: NodeKind,
}

struct DreamData {
    item: Item,
    label: String,
    strength: f64,
    provisional: bool,
    files: Vec<String>,
    neighbors: Vec<Neighbor>,
    sparkline: Vec<u64>,
    recall_total_30d: u64,
}

fn strength_label(s: f64) -> &'static str {
    if s >= 2.0 {
        "Strong"
    } else if s >= 1.5 {
        "Linked"
    } else {
        "Weak"
    }
}

/// Deterministic pseudo-noise (xorshift-style hash) — no rand dependency.
fn noise(seed: u64) -> u64 {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x
}

const NOISE_GLYPHS: &[char] = &[
    '░', '▒', '·', '∙', '˙', '¸', '˚', '⁚', '⋅', '∘', '°', '~',
];

fn item_label(item: &Item) -> String {
    format!(
        "{}{}",
        item.kind.chars().next().unwrap_or('m'),
        item.display_id.unwrap_or(0)
    )
}

fn load(conn: &Connection, handle: &str) -> Result<DreamData> {
    let item = db::get_item_by_handle(conn, handle)?;
    if item.kind != "memory" {
        anyhow::bail!("`sara dream` peeks into memories — {handle} is a {}", item.kind);
    }
    let label = item_label(&item);
    let strength = db::item_strength(conn, &item);
    let files = db::get_item_files(conn, &item.uuid).unwrap_or_default();

    let mut neighbors = vec![];
    let uuid_str = item.uuid.to_string();
    for link in db::get_memory_links_from(conn, &uuid_str).unwrap_or_default() {
        if let Ok(other) = db::get_item_by_uuid(conn, &link.to_uuid) {
            neighbors.push(Neighbor {
                kind: NodeKind::Memory { label: item_label(&other), relation: link.relation },
            });
        }
    }
    for link in db::get_memory_links_to(conn, &uuid_str).unwrap_or_default() {
        if let Ok(other) = db::get_item_by_uuid(conn, &link.from_uuid) {
            neighbors.push(Neighbor {
                kind: NodeKind::Memory {
                    label: item_label(&other),
                    relation: format!("⟵ {}", link.relation),
                },
            });
        }
    }
    for (task, source) in db::get_item_task_links(conn, &item.uuid).unwrap_or_default() {
        neighbors.push(Neighbor {
            kind: NodeKind::Task {
                id: task.id.map(|i| i.to_string()).unwrap_or_else(|| "?".into()),
                desc: task.description.clone(),
                source,
            },
        });
    }
    for f in &files {
        let name = f.rsplit('/').next().unwrap_or(f).to_string();
        neighbors.push(Neighbor { kind: NodeKind::File { name } });
    }

    let sparkline = db::memory_recall_daily_counts(conn, &item.uuid, 30);
    let recall_total_30d = sparkline.iter().sum();
    Ok(DreamData {
        provisional: item.status == "provisional",
        label,
        strength,
        files,
        neighbors,
        sparkline,
        recall_total_30d,
        item,
    })
}

pub fn run(conn: &Connection, handle: &str) -> Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return run_plain(conn, handle);
    }

    let mut data = load(conn, handle)?;
    // Peeking is a recall: reinforce. Fire-and-forget.
    let _ = db::record_memory_recall(conn, &data.item.uuid);

    let mut terminal = tui::init_terminal()?;
    let mut frame: u64 = 0;
    let mut selected: usize = 0;
    let mut breadcrumb: Vec<String> = vec![];
    let mut show_help = false;

    let res = loop {
        let draw = terminal.draw(|f| ui(f, &data, frame, selected, &breadcrumb, show_help));
        if let Err(e) = draw {
            break Err(e.into());
        }
        frame += 1;

        if crossterm::event::poll(std::time::Duration::from_millis(TICK_MS))? {
            use crossterm::event::{Event, KeyCode, KeyEventKind};
            if let Event::Key(key) = crossterm::event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') => break Ok(()),
                    KeyCode::Char('?') => show_help = !show_help,
                    KeyCode::Esc => {
                        if show_help {
                            show_help = false;
                        } else if let Some(prev) = breadcrumb.pop() {
                            if let Ok(d) = load(conn, &prev) {
                                data = d;
                                frame = 0;
                                selected = 0;
                            }
                        } else {
                            break Ok(());
                        }
                    }
                    KeyCode::Backspace => {
                        if let Some(prev) = breadcrumb.pop() {
                            if let Ok(d) = load(conn, &prev) {
                                data = d;
                                frame = 0;
                                selected = 0;
                            }
                        }
                    }
                    KeyCode::Tab | KeyCode::Right | KeyCode::Down => {
                        if !data.neighbors.is_empty() {
                            selected = (selected + 1) % data.neighbors.len();
                        }
                    }
                    KeyCode::BackTab | KeyCode::Left | KeyCode::Up => {
                        if !data.neighbors.is_empty() {
                            selected = (selected + data.neighbors.len() - 1) % data.neighbors.len();
                        }
                    }
                    KeyCode::Enter => {
                        // Drift along the selected dendrite — only memories are enterable.
                        if let Some(Neighbor { kind: NodeKind::Memory { label, .. } }) =
                            data.neighbors.get(selected)
                        {
                            let target = label.clone();
                            if let Ok(d) = load(conn, &target) {
                                breadcrumb.push(data.label.clone());
                                let _ = db::record_memory_recall(conn, &d.item.uuid);
                                data = d;
                                frame = 0;
                                selected = 0;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    };

    tui::restore_terminal()?;
    res
}

// ── rendering ────────────────────────────────────────────────────────────────

fn body_style(strength: f64, provisional: bool) -> Style {
    let mut style = if strength >= 2.0 {
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    } else if strength >= 1.5 {
        Style::default().fg(Color::Gray)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    if provisional {
        style = style.add_modifier(Modifier::ITALIC);
    }
    style
}

fn accent(provisional: bool) -> Color {
    if provisional { Color::Magenta } else { Color::Cyan }
}

/// Materialisation: how much of the body has resolved at this frame.
/// Weak memories surface slowly from the noise; strong ones snap into focus.
fn resolve_progress(frame: u64, strength: f64) -> f64 {
    let total_ticks = (100.0 - 25.0 * strength).max(30.0); // strong ≈ 50, weak ≈ 75
    (frame as f64 / total_ticks).min(1.0)
}

fn materialized_body(body: &str, frame: u64, strength: f64) -> Vec<(char, bool)> {
    let chars: Vec<char> = body.chars().collect();
    let progress = resolve_progress(frame, strength);
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            // Each char resolves at its own jittered threshold, so the text
            // condenses patchily rather than as a scanline.
            let jitter = (noise(i as u64) % 1000) as f64 / 1000.0;
            let threshold = 0.15 + 0.85 * jitter;
            if progress >= threshold || c == '\n' {
                (c, true)
            } else if c == ' ' {
                (' ', false)
            } else {
                let g = NOISE_GLYPHS
                    [(noise(i as u64 ^ (frame / 3)) % NOISE_GLYPHS.len() as u64) as usize];
                (g, false)
            }
        })
        .collect()
}

fn ui(
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

    // Breadcrumb / dream-path header.
    let mut path: Vec<Span> = vec![Span::styled(" ✦ ", Style::default().fg(ac))];
    for b in breadcrumb {
        path.push(Span::styled(b.clone(), Style::default().fg(Color::DarkGray)));
        path.push(Span::styled(" ⟶ ", Style::default().fg(Color::DarkGray)));
    }
    path.push(Span::styled(
        data.label.clone(),
        Style::default().fg(ac).add_modifier(Modifier::BOLD),
    ));
    if data.provisional {
        path.push(Span::styled(
            "  (provisional — an unreviewed dream)",
            Style::default().fg(Color::Magenta).add_modifier(Modifier::ITALIC),
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

fn render_neuron(f: &mut ratatui::Frame, area: Rect, data: &DreamData, frame: u64, selected: usize) {
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
            // Recall pulse: expanding ripples for the first PULSE_TICKS.
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
                // The observer effect: peeking reinforces.
                let float_y = 8.0 + t * 30.0;
                ctx.print(
                    6.0,
                    float_y,
                    Line::from(Span::styled(
                        "+0.1",
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                    )),
                );
            }

            // Dendrites + neighbor nodes on a circle around the cell body.
            for (i, nb) in data.neighbors.iter().enumerate() {
                let angle = std::f64::consts::TAU * (i as f64 / n.max(1) as f64)
                    + 0.35
                    + (frame as f64 * 0.004); // the whole web drifts, dreamlike
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

            // Cell body breathes: slow sine over luminosity + glyph.
            let breath = ((frame as f64 * 0.06).sin() + 1.0) / 2.0;
            let glyphs = ["◌", "○", "◎", "◉"];
            let gi = (breath * (glyphs.len() - 1) as f64).round() as usize;
            let lum = (120.0 + breath * 120.0 * (data.strength / 2.5).min(1.0)) as u8;
            let core_color = if data.provisional {
                Color::Rgb(lum, 60, lum)
            } else {
                Color::Rgb(60, lum, lum)
            };
            ctx.draw(&Circle { x: 0.0, y: 0.0, radius: 4.0 + breath * 2.0, color: core_color });
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

fn render_detail(f: &mut ratatui::Frame, area: Rect, data: &DreamData, frame: u64, selected: usize) {
    let ac = accent(data.provisional);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // strength gauge
            Constraint::Length(4), // recall pulse sparkline
            Constraint::Min(5),    // body
            Constraint::Length(4), // tags/files/selected
        ])
        .split(area);

    // Strength = how vividly this memory burns.
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
            .label(format!("{} ({:.1})", strength_label(data.strength), data.strength)),
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

    // The memory itself, condensing out of noise.
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
    let age = chrono::Utc::now().signed_duration_since(data.item.created).num_days();
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

    // Footer: tags + what the selected dendrite is.
    let mut foot: Vec<Line> = vec![Line::from(vec![
        Span::styled("tags ", Style::default().fg(Color::DarkGray)),
        Span::styled(data.item.tags.join(", "), Style::default().fg(Color::Yellow)),
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
        foot.push(Line::from(Span::styled(desc, Style::default().fg(Color::Gray))));
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

// ── plain fallback (non-TTY) ─────────────────────────────────────────────────

fn run_plain(conn: &Connection, handle: &str) -> Result<()> {
    let data = load(conn, handle)?;
    println!(
        "{} — {} ({:.1}){}",
        data.label,
        strength_label(data.strength),
        data.strength,
        if data.provisional { " [provisional]" } else { "" }
    );
    println!(
        "created: {}   recalls (30d): {}",
        data.item.created.to_rfc3339(),
        data.recall_total_30d
    );
    if !data.item.tags.is_empty() {
        println!("tags: {}", data.item.tags.join(", "));
    }
    if !data.files.is_empty() {
        println!("files: {}", data.files.join(", "));
    }
    for nb in &data.neighbors {
        match &nb.kind {
            NodeKind::Memory { label, relation } => println!("link: {relation} {label}"),
            NodeKind::Task { id, desc, source } => println!("task: #{id} ({source}) {desc}"),
            NodeKind::File { .. } => {}
        }
    }
    println!("\n{}", data.item.body);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materialization_starts_noisy_and_fully_resolves() {
        let body = "the quick brown fox";
        let early = materialized_body(body, 0, 1.0);
        let noisy = early.iter().filter(|(_, real)| !real).count();
        assert!(noisy > 0, "frame 0 should still contain noise");

        let late = materialized_body(body, 10_000, 1.0);
        assert!(late.iter().all(|(_, real)| *real), "must fully resolve");
        let text: String = late.iter().map(|(c, _)| c).collect();
        assert_eq!(text, body);
    }

    #[test]
    fn strong_memories_resolve_faster_than_weak() {
        let frame = 40;
        assert!(resolve_progress(frame, 2.5) > resolve_progress(frame, 1.0));
        assert!((resolve_progress(10_000, 1.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn strength_labels_match_thresholds() {
        assert_eq!(strength_label(2.0), "Strong");
        assert_eq!(strength_label(1.5), "Linked");
        assert_eq!(strength_label(1.49), "Weak");
    }

    #[test]
    fn noise_is_deterministic() {
        assert_eq!(noise(42), noise(42));
        assert_ne!(noise(1), noise(2));
    }
}
