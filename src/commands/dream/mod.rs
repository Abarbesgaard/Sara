use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::db;
use crate::infrastructure::memory::graph::MemoryGraph;
use crate::infrastructure::tui;

use crate::commands::shared::{canonical_labels, derived_from_suffix, item_label};
use std::collections::HashMap;

mod render;
mod types;
use types::{Assoc, Bond, Dir, DreamData, Neighbor, NodeKind, Star, WebData};

const TICK_MS: u64 = 50;
pub(super) const PULSE_TICKS: u64 = 40;

fn navigate_back(conn: &Connection, breadcrumb: &mut Vec<String>) -> Option<DreamData> {
    while let Some(prev) = breadcrumb.pop() {
        if let Ok(d) = load(conn, &prev) {
            return Some(d);
        }
    }
    None
}

fn load(conn: &Connection, handle: &str) -> Result<DreamData> {
    let item = db::get_item_by_handle(conn, handle)?;
    if item.kind != "memory" {
        anyhow::bail!(
            "`sara dream` peeks into memories — {handle} is a {}",
            item.kind
        );
    }
    let label = item_label(&item);
    let strength = db::item_strength(conn, &item);
    let files = db::get_item_files(conn, &item.uuid).unwrap_or_default();

    let mut neighbors = vec![];
    let uuid_str = item.uuid.to_string();
    for link in db::get_memory_links_from(conn, &uuid_str).unwrap_or_default() {
        if let Ok(other) = db::get_item_by_uuid(conn, &link.to_uuid) {
            neighbors.push(Neighbor {
                kind: NodeKind::Memory {
                    label: item_label(&other),
                    relation: link.relation,
                },
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
        neighbors.push(Neighbor {
            kind: NodeKind::File { name },
        });
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
    let _ = db::record_memory_recall(conn, &data.item.uuid);

    let mut terminal = tui::init_terminal()?;
    let mut frame: u64 = 0;
    let mut selected: usize = 0;
    let mut breadcrumb: Vec<String> = vec![];
    let mut show_help = false;

    let res = loop {
        let draw = terminal.draw(|f| render::ui(f, &data, frame, selected, &breadcrumb, show_help));
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
                        } else if breadcrumb.is_empty() {
                            break Ok(());
                        } else if let Some(d) = navigate_back(conn, &mut breadcrumb) {
                            data = d;
                            frame = 0;
                            selected = 0;
                        } else {
                            break Ok(());
                        }
                    }
                    KeyCode::Backspace => {
                        if let Some(d) = navigate_back(conn, &mut breadcrumb) {
                            data = d;
                            frame = 0;
                            selected = 0;
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
                        if let Some(Neighbor {
                            kind: NodeKind::Memory { label, .. },
                        }) = data.neighbors.get(selected)
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

fn load_web(conn: &Connection) -> Result<WebData> {
    let memories = db::list_memories(conn)?;
    let strengths = db::item_strengths(conn, &memories);
    let recent_counts = db::memory_recall_daily_counts_all(conn, 7);
    let mut index = HashMap::new();
    let mut stars: Vec<Star> = memories
        .iter()
        .enumerate()
        .map(|(i, m)| {
            index.insert(m.uuid.to_string(), i);
            let recent: u64 = recent_counts.get(&m.uuid).map_or(0, |c| c.iter().sum());
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
                strength: strengths.get(&m.uuid).copied().unwrap_or(1.0),
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
    let edges = MemoryGraph::build(conn)
        .map(|g| render::graph_edges(&g, &index))
        .unwrap_or_default();
    let affinity: Vec<(usize, usize, f64)> = edges
        .iter()
        .map(|&(a, b, w)| (a, b, render::spring_stiffness(w)))
        .collect();
    render::force_layout(&mut stars, &affinity, 250);
    let links: Vec<Assoc> = edges
        .into_iter()
        .map(|(a, b, weight)| Assoc { a, b, weight })
        .collect();
    Ok(WebData {
        stars,
        bonds,
        links,
    })
}

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
    let mut dream: Option<DreamData> = None;
    let mut dream_frame: u64 = 0;
    let mut dream_selected: usize = 0;
    let mut zoom: f64 = 1.0;
    let mut search_input: Option<String> = None;
    let mut query = String::new();

    let jump_to_match = |sel: usize, q: &str, stars: &[Star], back: bool| -> usize {
        if q.is_empty() {
            return sel;
        }
        let n = stars.len();
        for step in 1..=n {
            let i = if back {
                (sel + n - step) % n
            } else {
                (sel + step) % n
            };
            if stars[i].matches(q) {
                return i;
            }
        }
        sel
    };

    let res = loop {
        let draw = terminal.draw(|f| {
            if let Some(d) = &dream {
                render::ui(
                    f,
                    d,
                    dream_frame,
                    dream_selected,
                    &[web.stars[selected].label.clone()],
                    show_help,
                );
            } else {
                render::ui_web(
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
                if dream.is_none()
                    && let Some(buf) = &mut search_input
                {
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
                            dream = None;
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
                    KeyCode::Right => {
                        if let Some(d) = &dream {
                            if !d.neighbors.is_empty() {
                                dream_selected = (dream_selected + 1) % d.neighbors.len();
                            }
                        } else {
                            selected =
                                render::nearest_in_direction(&web.stars, selected, Dir::Right);
                        }
                    }
                    KeyCode::Down => {
                        if let Some(d) = &dream {
                            if !d.neighbors.is_empty() {
                                dream_selected = (dream_selected + 1) % d.neighbors.len();
                            }
                        } else {
                            selected =
                                render::nearest_in_direction(&web.stars, selected, Dir::Down);
                        }
                    }
                    KeyCode::Left => {
                        if let Some(d) = &dream {
                            if !d.neighbors.is_empty() {
                                dream_selected =
                                    (dream_selected + d.neighbors.len() - 1) % d.neighbors.len();
                            }
                        } else {
                            selected =
                                render::nearest_in_direction(&web.stars, selected, Dir::Left);
                        }
                    }
                    KeyCode::Up => {
                        if let Some(d) = &dream {
                            if !d.neighbors.is_empty() {
                                dream_selected =
                                    (dream_selected + d.neighbors.len() - 1) % d.neighbors.len();
                            }
                        } else {
                            selected = render::nearest_in_direction(&web.stars, selected, Dir::Up);
                        }
                    }
                    KeyCode::Enter => {
                        let target = if let Some(d) = &dream {
                            match d.neighbors.get(dream_selected) {
                                Some(Neighbor {
                                    kind: NodeKind::Memory { label, .. },
                                }) => Some(label.clone()),
                                _ => None,
                            }
                        } else {
                            Some(web.stars[selected].label.clone())
                        };
                        if let Some(t) = target
                            && let Ok(d) = load(conn, &t)
                        {
                            let _ = db::record_memory_recall(conn, &d.item.uuid);
                            dream = Some(d);
                            dream_frame = 0;
                            dream_selected = 0;
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

fn run_plain(conn: &Connection, handle: &str) -> Result<()> {
    let data = load(conn, handle)?;
    let _ = db::record_memory_recall(conn, &data.item.uuid);
    let (derived_labels, derived_from_labels) = canonical_labels(conn, &data.item);
    let canonical_str = if derived_labels.is_empty() {
        String::new()
    } else {
        format!(
            " [canonical, {} derived: {}]",
            derived_labels.len(),
            derived_labels.join(", ")
        )
    };
    let derived_from_str = derived_from_suffix(&derived_from_labels);
    println!(
        "{} — {} ({:.1}){}{}{}",
        data.label,
        render::strength_label(data.strength),
        data.strength,
        if data.provisional {
            " [provisional]"
        } else {
            ""
        },
        canonical_str,
        derived_from_str,
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
use render::{
    MIN_EDGE, bond_exists, force_layout, graph_edges, materialized_body, nearest_in_direction,
    noise, resolve_progress, spring_stiffness, strength_label,
};

#[cfg(test)]
#[path = "../../../tests/unit/commands/dream/mod.rs"]
mod tests;
