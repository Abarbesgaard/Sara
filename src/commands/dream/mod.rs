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
use crate::infrastructure::memory_graph::MemoryGraph;
use crate::infrastructure::model::Item;
use crate::infrastructure::tui;

use std::collections::HashMap;

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

// ── the web: whole-brain constellation view ──────────────────────────────────

struct Star {
    label: String,
    title: String,
    strength: f64,
    provisional: bool,
    tags: Vec<String>,
    /// Lowercased searchable text: label + tags + title + body.
    haystack: String,
    /// Canvas position, produced by the force layout.
    x: f64,
    y: f64,
    /// Recalled within the last 7 days — pulses.
    recently_recalled: bool,
}

impl Star {
    fn matches(&self, query: &str) -> bool {
        !query.is_empty() && self.haystack.contains(query)
    }
}

struct Bond {
    a: usize,
    b: usize,
    relation: String,
}

/// A calibrated shared-anchor association between two stars, drawn as a faint
/// thread whose brightness tracks `weight` (0..=1). Distinct from a [`Bond`],
/// which is an explicit, authored `memory_link`.
struct Assoc {
    a: usize,
    b: usize,
    weight: f64,
}

struct WebData {
    stars: Vec<Star>,
    bonds: Vec<Bond>,
    links: Vec<Assoc>,
}

/// True if an explicit authored bond already connects stars `a` and `b` (either
/// direction) — used to avoid drawing a faint association thread under a bond.
fn bond_exists(bonds: &[Bond], a: usize, b: usize) -> bool {
    bonds.iter().any(|bd| (bd.a == a && bd.b == b) || (bd.a == b && bd.b == a))
}

/// A screen direction for spatial navigation across the constellation.
#[derive(Clone, Copy)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// The star nearest to `from` in screen-direction `dir`. Only stars that lie
/// genuinely that way are eligible; among them the closest wins, with sideways
/// drift penalised so `→` favours a star to the right over one far above. Falls
/// back to the current star if nothing lies in that direction (edge of the web).
fn nearest_in_direction(stars: &[Star], from: usize, dir: Dir) -> usize {
    let (ox, oy) = (stars[from].x, stars[from].y);
    let mut best = from;
    let mut best_score = f64::MAX;
    for (i, s) in stars.iter().enumerate() {
        if i == from {
            continue;
        }
        let (dx, dy) = (s.x - ox, s.y - oy);
        // Component along the travel axis (must be forward) and perpendicular.
        let (along, perp) = match dir {
            Dir::Right => (dx, dy.abs()),
            Dir::Left => (-dx, dy.abs()),
            Dir::Up => (dy, dx.abs()),
            Dir::Down => (-dy, dx.abs()),
        };
        if along <= 0.0 {
            continue;
        }
        // Prefer straight-ahead: distance along the axis plus a heavy sideways
        // penalty so the cone stays narrow.
        let score = along + 2.0 * perp;
        if score < best_score {
            best_score = score;
            best = i;
        }
    }
    best
}

/// Force-layout tuning for the constellation. The memory graph hands us
/// synapse weights in `0..=1`; these turn them into spring stiffnesses that
/// separate the web into visible clusters instead of one uniform ball:
///
/// * `MIN_EDGE` — drop synapses below this weight. A tag shared across a third
///   of the store carries almost no associative signal (IDF → ~0); keeping its
///   spring just re-clumps everything. Cutting it lets real associations shape
///   the layout.
/// * `CONTRAST` — raise weights to this power before scaling. Strong, specific
///   links stay near their value while weak ones collapse toward zero, so a
///   rare shared anchor binds *dramatically* tighter than a common one.
/// * `AFFINITY_SCALE` — final stiffness of a full-strength synapse. Enough to
///   snap a tightly-bound pair together against the layout's repulsion.
const MIN_EDGE: f64 = 0.15;
const CONTRAST: f64 = 3.0;
const AFFINITY_SCALE: f64 = 0.12;

/// Per-pair repulsion strength (`REPULSION / distance²`) and centre-seeking
/// gravity. Tuned together so ~150 stars settle into an evenly-spread island
/// that floats clear of the canvas walls instead of jamming against them.
const REPULSION: f64 = 190.0;
const GRAVITY: f64 = 0.14;

/// The calibrated associations between memories: each `(star_i, star_j,
/// weight)` is a synapse from the memory graph — IDF-weighted shared anchors
/// (tags/files/tasks) plus relation-weighted `memory_links` — filtered to those
/// carrying real signal ([`MIN_EDGE`]). This is the single source of
/// associative truth (the same graph recall spreads activation over); the web
/// both *lays out* stars by these weights and *draws* them as faint threads, so
/// what pulls two memories together is also what you see connecting them.
fn graph_edges(graph: &MemoryGraph, index: &HashMap<String, usize>) -> Vec<(usize, usize, f64)> {
    graph
        .edges()
        .into_iter()
        .filter(|(_, _, w)| *w >= MIN_EDGE)
        .filter_map(|(a, b, w)| {
            let ia = *index.get(&a.to_string())?;
            let ib = *index.get(&b.to_string())?;
            Some((ia, ib, w))
        })
        .collect()
}

/// Turn a synapse weight (0..=1) into a force-layout spring stiffness. Strong,
/// specific links stay near their value while weak ones collapse toward zero
/// ([`CONTRAST`]), so a rare shared anchor binds dramatically tighter than a
/// common one; [`AFFINITY_SCALE`] sets the absolute pull.
fn spring_stiffness(weight: f64) -> f64 {
    weight.powf(CONTRAST) * AFFINITY_SCALE
}

/// Force-directed layout: golden-angle spiral seed, then a few hundred
/// relaxation steps. Repulsion pushes every star apart; the affinity springs
/// (from the memory graph's calibrated synapses) pull associated memories
/// together into clusters; gravity pulls the whole web toward the centre.
///
/// [`REPULSION`] and [`GRAVITY`] are balanced so the graph settles into an
/// evenly-spread island that floats clear of the canvas edges — like an
/// Obsidian graph view — rather than a ball that flies outward and jams
/// against the boundary walls.
fn force_layout(stars: &mut [Star], affinity: &[(usize, usize, f64)], iterations: usize) {
    let n = stars.len();
    if n < 2 {
        return;
    }
    // Golden-angle spiral seed for an even initial spread.
    for (i, s) in stars.iter_mut().enumerate() {
        let r = 70.0 * ((i + 1) as f64 / n as f64).sqrt();
        let a = i as f64 * 2.399_963; // golden angle
        s.x = a.cos() * r * 1.3;
        s.y = a.sin() * r;
    }

    for _ in 0..iterations {
        let mut fx = vec![0.0f64; n];
        let mut fy = vec![0.0f64; n];
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = stars[i].x - stars[j].x;
                let dy = stars[i].y - stars[j].y;
                let d2 = (dx * dx + dy * dy).max(4.0);
                let rep = REPULSION / d2;
                let d = d2.sqrt();
                fx[i] += dx / d * rep;
                fy[i] += dy / d * rep;
                fx[j] -= dx / d * rep;
                fy[j] -= dy / d * rep;
            }
            // Gravity toward centre — strong enough to keep the web off the walls.
            fx[i] -= stars[i].x * GRAVITY;
            fy[i] -= stars[i].y * GRAVITY;
        }
        for &(i, j, k) in affinity {
            let dx = stars[j].x - stars[i].x;
            let dy = stars[j].y - stars[i].y;
            fx[i] += dx * k;
            fy[i] += dy * k;
            fx[j] -= dx * k;
            fy[j] -= dy * k;
        }
        for i in 0..n {
            stars[i].x = (stars[i].x + fx[i].clamp(-4.0, 4.0)).clamp(-95.0, 95.0);
            stars[i].y = (stars[i].y + fy[i].clamp(-4.0, 4.0)).clamp(-68.0, 68.0);
        }
    }
}

fn load_web(conn: &Connection) -> Result<WebData> {
    let memories = db::list_memories(conn)?;
    let mut index = HashMap::new();
    let mut stars: Vec<Star> = memories
        .iter()
        .enumerate()
        .map(|(i, m)| {
            index.insert(m.uuid.to_string(), i);
            let recent: u64 = db::memory_recall_daily_counts(conn, &m.uuid, 7).iter().sum();
            let label = item_label(m);
            let haystack = format!(
                "{} {} {} {}",
                label.to_lowercase(),
                m.tags.join(" ").to_lowercase(),
                m.title.to_lowercase(),
                m.body.to_lowercase()
            );
            Star {
                label,
                title: m.title.clone(),
                strength: db::item_strength(conn, m),
                provisional: m.status == "provisional",
                tags: m.tags.clone(),
                haystack,
                x: 0.0,
                y: 0.0,
                recently_recalled: recent > 0,
            }
        })
        .collect();
    let bonds: Vec<Bond> = db::all_memory_links(conn)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|l| {
            Some(Bond {
                a: *index.get(&l.from_uuid)?,
                b: *index.get(&l.to_uuid)?,
                relation: l.relation,
            })
        })
        .collect();
    // Drive the layout from the calibrated nervous-system graph rather than a
    // flat shared-tag/uniform-bond heuristic, so the constellation reflects the
    // same associations recall spreads over. The same edges are kept as faint
    // threads (`links`) so what pulls stars together is also what's drawn.
    // Falls back to no springs/threads if the graph can't be built.
    let edges = MemoryGraph::build(conn)
        .map(|g| graph_edges(&g, &index))
        .unwrap_or_default();
    let affinity: Vec<(usize, usize, f64)> =
        edges.iter().map(|&(a, b, w)| (a, b, spring_stiffness(w))).collect();
    force_layout(&mut stars, &affinity, 250);
    let links: Vec<Assoc> = edges.into_iter().map(|(a, b, weight)| Assoc { a, b, weight }).collect();
    Ok(WebData { stars, bonds, links })
}

/// `sara dream` with no label: the whole brain at once. Enter dives into the
/// neuron view for the selected star; Esc zooms back out to the web.
pub fn run_web(conn: &Connection) -> Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return run_web_plain(conn);
    }
    let web = load_web(conn)?;
    if web.stars.is_empty() {
        println!("No memories yet — nothing to dream about. Use `sara learn` first.");
        return Ok(());
    }

    let mut terminal = tui::init_terminal()?;
    let mut frame: u64 = 0;
    let mut selected: usize = 0;
    let mut show_help = false;
    // When Some, we've dived into a single neuron.
    let mut dream: Option<DreamData> = None;
    let mut dream_frame: u64 = 0;
    let mut dream_selected: usize = 0;
    // Zoom factor for the web canvas; view drifts toward the selected star.
    let mut zoom: f64 = 1.0;
    // Some(buffer) while typing a search; `query` is the committed filter.
    let mut search_input: Option<String> = None;
    let mut query = String::new();

    let jump_to_match = |sel: usize, q: &str, stars: &[Star], back: bool| -> usize {
        if q.is_empty() {
            return sel;
        }
        let n = stars.len();
        for step in 1..=n {
            let i = if back { (sel + n - step) % n } else { (sel + step) % n };
            if stars[i].matches(q) {
                return i;
            }
        }
        sel
    };

    let res = loop {
        let draw = terminal.draw(|f| {
            if let Some(d) = &dream {
                ui(f, d, dream_frame, dream_selected, &[web.stars[selected].label.clone()], show_help);
            } else {
                ui_web(
                    f,
                    &web,
                    frame,
                    selected,
                    show_help,
                    zoom,
                    search_input.as_deref(),
                    &query,
                );
            }
        });
        if let Err(e) = draw {
            break Err(e.into());
        }
        frame += 1;
        dream_frame += 1;

        if crossterm::event::poll(std::time::Duration::from_millis(TICK_MS))? {
            use crossterm::event::{Event, KeyCode, KeyEventKind};
            if let Event::Key(key) = crossterm::event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                // Search-typing mode swallows most keys.
                if dream.is_none() {
                    if let Some(buf) = &mut search_input {
                        match key.code {
                            KeyCode::Esc => {
                                search_input = None;
                                query.clear();
                            }
                            KeyCode::Enter => {
                                query = buf.trim().to_lowercase();
                                search_input = None;
                                selected = jump_to_match(selected, &query, &web.stars, false);
                            }
                            KeyCode::Backspace => {
                                if buf.pop().is_none() {
                                    search_input = None;
                                }
                            }
                            KeyCode::Char(c) => buf.push(c),
                            _ => {}
                        }
                        continue;
                    }
                }
                match key.code {
                    KeyCode::Char('q') => break Ok(()),
                    KeyCode::Char('?') => show_help = !show_help,
                    KeyCode::Char('/') if dream.is_none() => {
                        search_input = Some(String::new());
                    }
                    KeyCode::Char('n') if dream.is_none() => {
                        selected = jump_to_match(selected, &query, &web.stars, false);
                    }
                    KeyCode::Char('N') if dream.is_none() => {
                        selected = jump_to_match(selected, &query, &web.stars, true);
                    }
                    KeyCode::Char('+') | KeyCode::Char('=') if dream.is_none() => {
                        zoom = (zoom * 1.25).min(6.0);
                    }
                    KeyCode::Char('-') if dream.is_none() => {
                        zoom = (zoom / 1.25).max(1.0);
                    }
                    KeyCode::Char('0') if dream.is_none() => zoom = 1.0,
                    KeyCode::Esc | KeyCode::Backspace => {
                        if show_help {
                            show_help = false;
                        } else if dream.is_some() {
                            dream = None; // zoom back out to the web
                        } else if !query.is_empty() {
                            query.clear();
                        } else if zoom > 1.0 {
                            zoom = 1.0;
                        } else {
                            break Ok(());
                        }
                    }
                    KeyCode::Tab => {
                        if let Some(d) = &dream {
                            if !d.neighbors.is_empty() {
                                dream_selected = (dream_selected + 1) % d.neighbors.len();
                            }
                        } else {
                            selected = (selected + 1) % web.stars.len();
                        }
                    }
                    KeyCode::BackTab => {
                        if let Some(d) = &dream {
                            if !d.neighbors.is_empty() {
                                dream_selected =
                                    (dream_selected + d.neighbors.len() - 1) % d.neighbors.len();
                            }
                        } else {
                            selected = (selected + web.stars.len() - 1) % web.stars.len();
                        }
                    }
                    // Arrows: in the neuron view, cycle dendrites; in the web,
                    // glide the selection (and camera) to the nearest star that
                    // way, so navigation feels spatial rather than by index.
                    KeyCode::Right => {
                        if let Some(d) = &dream {
                            if !d.neighbors.is_empty() {
                                dream_selected = (dream_selected + 1) % d.neighbors.len();
                            }
                        } else {
                            selected = nearest_in_direction(&web.stars, selected, Dir::Right);
                        }
                    }
                    KeyCode::Down => {
                        if let Some(d) = &dream {
                            if !d.neighbors.is_empty() {
                                dream_selected = (dream_selected + 1) % d.neighbors.len();
                            }
                        } else {
                            selected = nearest_in_direction(&web.stars, selected, Dir::Down);
                        }
                    }
                    KeyCode::Left => {
                        if let Some(d) = &dream {
                            if !d.neighbors.is_empty() {
                                dream_selected =
                                    (dream_selected + d.neighbors.len() - 1) % d.neighbors.len();
                            }
                        } else {
                            selected = nearest_in_direction(&web.stars, selected, Dir::Left);
                        }
                    }
                    KeyCode::Up => {
                        if let Some(d) = &dream {
                            if !d.neighbors.is_empty() {
                                dream_selected =
                                    (dream_selected + d.neighbors.len() - 1) % d.neighbors.len();
                            }
                        } else {
                            selected = nearest_in_direction(&web.stars, selected, Dir::Up);
                        }
                    }
                    KeyCode::Enter => {
                        let target = if let Some(d) = &dream {
                            // Drift within the neuron view along a memory dendrite.
                            match d.neighbors.get(dream_selected) {
                                Some(Neighbor { kind: NodeKind::Memory { label, .. } }) => {
                                    Some(label.clone())
                                }
                                _ => None,
                            }
                        } else {
                            Some(web.stars[selected].label.clone())
                        };
                        if let Some(t) = target {
                            if let Ok(d) = load(conn, &t) {
                                let _ = db::record_memory_recall(conn, &d.item.uuid);
                                dream = Some(d);
                                dream_frame = 0;
                                dream_selected = 0;
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

#[allow(clippy::too_many_arguments)]
fn ui_web(
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
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let strong = web.stars.iter().filter(|s| s.strength >= 2.0).count();
    let linked = web.stars.iter().filter(|s| s.strength >= 1.5 && s.strength < 2.0).count();
    let weak = web.stars.len() - strong - linked;
    let matches = web.stars.iter().filter(|s| s.matches(query)).count();
    let mut header = vec![
        Span::styled(" ✦ the web ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(
                "— {} memories · {} bonds · ",
                web.stars.len(),
                web.bonds.len()
            ),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(format!("{strong} strong "), Style::default().fg(Color::White)),
        Span::styled(format!("{linked} linked "), Style::default().fg(Color::Cyan)),
        Span::styled(format!("{weak} weak"), Style::default().fg(Color::DarkGray)),
    ];
    if let Some(buf) = search_input {
        header.push(Span::styled(
            format!("   /{buf}▌"),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
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

    // Zoom transform: the view drifts toward the selected star as zoom grows
    // (at 1x the centre stays at the origin, so the whole web is visible).
    let sel_star = &web.stars[selected];
    let (cx, cy) = (
        sel_star.x * (1.0 - 1.0 / zoom),
        sel_star.y * (1.0 - 1.0 / zoom),
    );
    let tx = move |x: f64| (x - cx) * zoom;
    let ty = move |y: f64| (y - cy) * zoom;

    let searching = !query.is_empty();
    let canvas = Canvas::default()
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)))
        .x_bounds([-100.0, 100.0])
        .y_bounds([-75.0, 75.0])
        .paint(move |ctx| {
            // Faint association threads underneath everything: the shared-anchor
            // synapses that actually pull the layout together. Skip pairs that
            // also have an explicit bond (drawn brighter, just below). Brightness
            // tracks synapse weight; the selected star's threads warm up.
            for l in &web.links {
                if bond_exists(&web.bonds, l.a, l.b) {
                    continue;
                }
                let (sa, sb) = (&web.stars[l.a], &web.stars[l.b]);
                let touches_sel = l.a == selected || l.b == selected;
                let faded = searching && !(sa.matches(query) || sb.matches(query));
                // weight 0.15..1.0 → dim..less-dim grey; selected neighbourhood tints teal.
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
            // Explicit authored bonds on top of the threads, stars on top of all.
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
                // Strength → glyph + luminosity; recent recalls pulse.
                let pulse = if s.recently_recalled { breath } else { 0.35 };
                let lum = (70.0 + 150.0 * (s.strength / 2.5).min(1.0) * (0.55 + 0.45 * pulse)) as u8;
                let (glyph, color) = if s.provisional {
                    ("◌", Color::Rgb(lum, 50, lum))
                } else if s.strength >= 2.0 {
                    ("✦", Color::Rgb(lum, lum, lum))
                } else if s.strength >= 1.5 {
                    ("✧", Color::Rgb(50, lum, lum))
                } else {
                    ("·", Color::Rgb(lum / 2, lum / 2, (lum as u16 + 30).min(255) as u8))
                };
                let style = if is_sel {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else if searching && is_match {
                    Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)
                } else if searching {
                    // Non-matching stars sink into the fog.
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

    // Footer: the selected star.
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
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} ({:.1}){} ", strength_label(s.strength), s.strength,
                if s.provisional { " [provisional]" } else { "" }),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(format!("[{}]  ", s.tags.join(", ")), Style::default().fg(Color::Yellow)),
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
            Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)),
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

fn run_web_plain(conn: &Connection) -> Result<()> {
    let web = load_web(conn)?;
    println!("{} memories, {} bonds", web.stars.len(), web.bonds.len());
    for b in &web.bonds {
        println!(
            "{} —[{}]→ {}",
            web.stars[b.a].label, b.relation, web.stars[b.b].label
        );
    }
    Ok(())
}

// ── plain fallback (non-TTY) ─────────────────────────────────────────────────

fn run_plain(conn: &Connection, handle: &str) -> Result<()> {
    let data = load(conn, handle)?;
    // Peeking is a recall: reinforce, exactly as the TTY path does. Scripted /
    // piped reads must strengthen a memory too, or automation silently starves
    // the usage signal that lifts a Weak memory to Linked. Fire-and-forget.
    let _ = db::record_memory_recall(conn, &data.item.uuid);
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

    #[test]
    fn force_layout_pulls_bonded_stars_closer_than_strangers() {
        let mk = |tags: &[&str]| Star {
            label: "m1".into(),
            title: String::new(),
            strength: 1.0,
            provisional: false,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            haystack: String::new(),
            x: 0.0,
            y: 0.0,
            recently_recalled: false,
        };
        let mut stars = vec![mk(&["a"]), mk(&["a"]), mk(&["b"]), mk(&["c"])];
        // A firm spring between stars 0 and 1; strangers (2,3) share nothing.
        let affinity = vec![(0usize, 1usize, 0.02f64)];
        force_layout(&mut stars, &affinity, 250);
        let d = |i: usize, j: usize| {
            let (dx, dy) = (stars[i].x - stars[j].x, stars[i].y - stars[j].y);
            (dx * dx + dy * dy).sqrt()
        };
        assert!(d(0, 1) < d(2, 3), "sprung pair should sit closer than strangers");
        // Everything stays inside the canvas.
        assert!(stars.iter().all(|s| s.x.abs() <= 95.0 && s.y.abs() <= 68.0));
    }

    #[test]
    fn affinity_springs_derive_from_calibrated_graph_weights() {
        use crate::infrastructure::model::Item;
        let conn = db::open_in_memory_for_test();
        let seed = |tags: &[&str]| -> String {
            let mut item = Item::new_memory("t".into(), "b".into(), None);
            item.tags = tags.iter().map(|s| s.to_string()).collect();
            item.path = Some(String::new());
            db::insert_item(&conn, &mut item).unwrap();
            item.uuid.to_string()
        };
        // 'common' is on many memories (ubiquitous → low IDF); 'rare' is only on
        // the a–b pair (high IDF → binds tighter).
        let a = seed(&["common", "rare"]);
        let b = seed(&["rare"]);
        let c = seed(&["common"]);
        for _ in 0..6 {
            seed(&["common"]);
        }

        let mut index = HashMap::new();
        for (i, m) in db::list_memories(&conn).unwrap().iter().enumerate() {
            index.insert(m.uuid.to_string(), i);
        }
        let graph = MemoryGraph::build(&conn).unwrap();
        let affinity: Vec<(usize, usize, f64)> = graph_edges(&graph, &index)
            .into_iter()
            .map(|(a, b, w)| (a, b, spring_stiffness(w)))
            .collect();

        let spring = |u: &str, v: &str| -> f64 {
            let (iu, iv) = (index[u], index[v]);
            affinity
                .iter()
                .find(|(i, j, _)| (*i == iu && *j == iv) || (*i == iv && *j == iu))
                .map(|(_, _, k)| *k)
                .unwrap_or(0.0)
        };
        let rare_pair = spring(&a, &b);
        let common_pair = spring(&a, &c);
        assert!(rare_pair > 0.0, "rare-anchor pair must keep a real spring");
        assert!(
            rare_pair > common_pair,
            "rare shared anchor ({rare_pair}) must bind tighter than ubiquitous ({common_pair}); \
             ubiquitous anchors below MIN_EDGE are dropped to 0 to de-clump the web",
        );
    }

    #[test]
    fn shared_anchor_pairs_become_association_threads() {
        use crate::infrastructure::model::Item;
        let conn = db::open_in_memory_for_test();
        let seed = |tags: &[&str]| {
            let mut item = Item::new_memory("t".into(), "b".into(), None);
            item.tags = tags.iter().map(|s| s.to_string()).collect();
            item.path = Some(String::new());
            db::insert_item(&conn, &mut item).unwrap();
        };
        // Two memories share a rare tag but have NO explicit memory_link;
        // padding memories make the tag genuinely rare (df < n, so idf > 0).
        seed(&["rare"]);
        seed(&["rare"]);
        for _ in 0..6 {
            seed(&["filler"]);
        }

        let web = load_web(&conn).unwrap();
        assert!(web.bonds.is_empty(), "no explicit links were authored");
        assert!(
            !web.links.is_empty(),
            "a shared-anchor association must surface as a drawable thread even without a bond",
        );
        assert!(
            web.links.iter().all(|l| l.weight >= MIN_EDGE),
            "only real-signal associations (>= MIN_EDGE) are kept as threads",
        );
    }

    #[test]
    fn bond_exists_matches_either_direction() {
        let bonds = vec![Bond { a: 1, b: 4, relation: "similar_to".into() }];
        assert!(bond_exists(&bonds, 1, 4));
        assert!(bond_exists(&bonds, 4, 1));
        assert!(!bond_exists(&bonds, 1, 2));
    }

    #[test]
    fn nearest_in_direction_picks_the_star_that_way() {
        let mk = |x: f64, y: f64| Star {
            label: String::new(),
            title: String::new(),
            strength: 1.0,
            provisional: false,
            tags: vec![],
            haystack: String::new(),
            x,
            y,
            recently_recalled: false,
        };
        // 0 at origin; 1 right, 2 left, 3 up, 4 down.
        let stars = vec![mk(0.0, 0.0), mk(10.0, 0.0), mk(-10.0, 0.0), mk(0.0, 10.0), mk(0.0, -10.0)];
        assert_eq!(nearest_in_direction(&stars, 0, Dir::Right), 1);
        assert_eq!(nearest_in_direction(&stars, 0, Dir::Left), 2);
        assert_eq!(nearest_in_direction(&stars, 0, Dir::Up), 3);
        assert_eq!(nearest_in_direction(&stars, 0, Dir::Down), 4);
        // Nothing to the right of the right-most star → stays put.
        assert_eq!(nearest_in_direction(&stars, 1, Dir::Right), 1);
        // A near star straight ahead beats a far one off to the side.
        let stars2 = vec![mk(0.0, 0.0), mk(5.0, 1.0), mk(6.0, 40.0)];
        assert_eq!(nearest_in_direction(&stars2, 0, Dir::Right), 1);
    }

    #[test]
    fn star_matches_searches_haystack_and_ignores_empty_query() {
        let star = Star {
            label: "m49".into(),
            title: "Usage-based strength".into(),
            strength: 1.0,
            provisional: false,
            tags: vec!["memory".into()],
            haystack: "m49 memory usage-based strength recall boost".into(),
            x: 0.0,
            y: 0.0,
            recently_recalled: false,
        };
        assert!(star.matches("recall"));
        assert!(star.matches("m49"));
        assert!(!star.matches("payment"));
        assert!(!star.matches(""), "empty query must not light everything up");
    }
}
