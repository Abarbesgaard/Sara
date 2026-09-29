use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Sparkline, Wrap};

use std::collections::HashMap;

use crate::infrastructure::memory_graph::MemoryGraph;
use crate::infrastructure::tui;

use super::PULSE_TICKS;

use super::types::{Bond, Dir, DreamData, NodeKind, Star, WebData};

pub(super) fn strength_label(s: f64) -> &'static str {
    if s >= 2.0 {
        "Strong"
    } else if s >= 1.5 {
        "Linked"
    } else {
        "Weak"
    }
}

/// Deterministic pseudo-noise (xorshift-style hash) — no rand dependency.
pub(super) fn noise(seed: u64) -> u64 {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x
}

pub(super) const NOISE_GLYPHS: &[char] =
    &['░', '▒', '·', '∙', '˙', '¸', '˚', '⁚', '⋅', '∘', '°', '~'];

pub(super) fn body_style(strength: f64, provisional: bool) -> Style {
    let mut style = if strength >= 2.0 {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
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

pub(super) fn accent(provisional: bool) -> Color {
    if provisional {
        Color::Magenta
    } else {
        Color::Cyan
    }
}

/// Materialisation: how much of the body has resolved at this frame.
/// Weak memories surface slowly from the noise; strong ones snap into focus.
pub(super) fn resolve_progress(frame: u64, strength: f64) -> f64 {
    let total_ticks = (100.0 - 25.0 * strength).max(30.0); // strong ≈ 50, weak ≈ 75
    (frame as f64 / total_ticks).min(1.0)
}

pub(super) fn materialized_body(body: &str, frame: u64, strength: f64) -> Vec<(char, bool)> {
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

pub(super) fn ui(
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

pub(super) fn render_neuron(
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
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
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

pub(super) fn render_detail(
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

    // Footer: tags + what the selected dendrite is.
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

/// True if an explicit authored bond already connects stars `a` and `b` (either
/// direction) — used to avoid drawing a faint association thread under a bond.
pub(super) fn bond_exists(bonds: &[Bond], a: usize, b: usize) -> bool {
    bonds
        .iter()
        .any(|bd| (bd.a == a && bd.b == b) || (bd.a == b && bd.b == a))
}

/// The star nearest to `from` in screen-direction `dir`. Only stars that lie
/// genuinely that way are eligible; among them the closest wins, with sideways
/// drift penalised so `→` favours a star to the right over one far above. Falls
/// back to the current star if nothing lies in that direction (edge of the web).
pub(super) fn nearest_in_direction(stars: &[Star], from: usize, dir: Dir) -> usize {
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
pub(super) const MIN_EDGE: f64 = 0.15;

pub(super) const CONTRAST: f64 = 3.0;

pub(super) const AFFINITY_SCALE: f64 = 0.12;

/// Per-pair repulsion strength (`REPULSION / distance²`) and centre-seeking
/// gravity. Tuned together so ~150 stars settle into an evenly-spread island
/// that floats clear of the canvas walls instead of jamming against them.
pub(super) const REPULSION: f64 = 190.0;

pub(super) const GRAVITY: f64 = 0.14;

/// The calibrated associations between memories: each `(star_i, star_j,
/// weight)` is a synapse from the memory graph — IDF-weighted shared anchors
/// (tags/files/tasks) plus relation-weighted `memory_links` — filtered to those
/// carrying real signal ([`MIN_EDGE`]). This is the single source of
/// associative truth (the same graph recall spreads activation over); the web
/// both *lays out* stars by these weights and *draws* them as faint threads, so
/// what pulls two memories together is also what you see connecting them.
pub(super) fn graph_edges(
    graph: &MemoryGraph,
    index: &HashMap<String, usize>,
) -> Vec<(usize, usize, f64)> {
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
pub(super) fn spring_stiffness(weight: f64) -> f64 {
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
pub(super) fn force_layout(
    stars: &mut [Star],
    affinity: &[(usize, usize, f64)],
    iterations: usize,
) {
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

#[allow(clippy::too_many_arguments)]
pub(super) fn ui_web(
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
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
