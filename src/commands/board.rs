use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use rusqlite::Connection;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use uuid::Uuid;

use crate::config::Config;
use crate::db;
use crate::model::{Priority, Status, Task};
use crate::project::detect_current_project;
use crate::tui;

enum BoardAction {
    Quit,
    OpenTask(String),
}

/// A feature = a chain of tasks linked by `sara dep` dependencies (one connected
/// component of the dependency graph). Standalone tasks land in a trailing
/// pseudo-feature with `grouped == false`.
struct Feature {
    title: String,
    done: usize,
    total: usize,
    grouped: bool,
}

struct BoardState {
    project: String,
    /// Tasks in feature-grouped, dependency (blockers-first) order.
    tasks: Vec<Task>,
    /// Feature index for each task in `tasks`.
    feature_of: Vec<usize>,
    features: Vec<Feature>,
    selected: usize,
    scroll: u16,
}

pub fn run(conn: &Connection, cfg: &Config, project_arg: Option<&str>) -> Result<()> {
    let project = if let Some(p) = project_arg {
        p.to_string()
    } else {
        let (name, _) = detect_current_project(conn, cfg)?;
        name
    };

    let mut st = build_state(conn, project)?;
    if st.tasks.is_empty() {
        println!("No tasks for project '{}'.", st.project);
        return Ok(());
    }

    loop {
        let mut terminal = tui::init_terminal()?;
        let action = board_loop(&mut terminal, &mut st)?;
        tui::restore_terminal()?;

        match action {
            BoardAction::Quit => break,
            BoardAction::OpenTask(uuid) => {
                crate::commands::info::run(conn, cfg, &uuid)?;
                // Reload — status/dependencies may have changed in the detail view.
                let project = std::mem::take(&mut st.project);
                let sel = st.selected;
                st = build_state(conn, project)?;
                if st.tasks.is_empty() {
                    break;
                }
                st.selected = sel.min(st.tasks.len() - 1);
            }
        }
    }
    Ok(())
}

/// Load tasks for the project and group them into features (dependency chains).
fn build_state(conn: &Connection, project: String) -> Result<BoardState> {
    let all = db::list_tasks_for_board(conn, &project)?;
    let edges = db::dependency_edges_for_project(conn, &project)?;

    // uuid -> position in `all`
    let pos: HashMap<Uuid, usize> = all.iter().enumerate().map(|(i, t)| (t.uuid, i)).collect();
    let n = all.len();

    // Union-find over task positions; union the two endpoints of every edge.
    let mut parent: Vec<usize> = (0..n).collect();
    // Execution-order adjacency (blocker -> dependent) + in-degree for topo sort.
    let mut dependents: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut indeg = vec![0usize; n];
    for (task, dep) in &edges {
        if let (Some(&ti), Some(&di)) = (pos.get(task), pos.get(dep)) {
            union(&mut parent, ti, di);
            dependents.entry(di).or_default().push(ti);
            indeg[ti] += 1;
        }
    }

    // Bucket task positions by their connected-component root.
    let mut comps: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        comps.entry(r).or_default().push(i);
    }

    // Split into multi-task features and singleton (ungrouped) tasks.
    let mut features_nodes: Vec<Vec<usize>> = Vec::new();
    let mut ungrouped: Vec<usize> = Vec::new();
    for nodes in comps.into_values() {
        if nodes.len() >= 2 {
            features_nodes.push(topo_order(&nodes, &dependents, &indeg));
        } else {
            ungrouped.push(nodes[0]);
        }
    }

    // Order features: active (has pending work) first, by best pending urgency.
    features_nodes.sort_by(|a, b| {
        feature_sort_key(b, &all)
            .partial_cmp(&feature_sort_key(a, &all))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Singletons keep `all` order: pending by urgency, then completed by end.
    ungrouped.sort_unstable();

    // Flatten into the render order, tagging each task with its feature index.
    let mut tasks: Vec<Task> = Vec::with_capacity(n);
    let mut feature_of: Vec<usize> = Vec::with_capacity(n);
    let mut features: Vec<Feature> = Vec::new();

    for nodes in &features_nodes {
        let fi = features.len();
        let total = nodes.len();
        let done = nodes
            .iter()
            .filter(|&&i| all[i].status == Status::Completed)
            .count();
        // Name the feature after its final (goal) task — last in chain order.
        let title = nodes
            .last()
            .map(|&i| truncate(&all[i].description, 56))
            .unwrap_or_else(|| format!("Feature {}", fi + 1));
        features.push(Feature {
            title,
            done,
            total,
            grouped: true,
        });
        for &i in nodes {
            tasks.push(all[i].clone());
            feature_of.push(fi);
        }
    }

    if !ungrouped.is_empty() {
        let fi = features.len();
        let total = ungrouped.len();
        let done = ungrouped
            .iter()
            .filter(|&&i| all[i].status == Status::Completed)
            .count();
        features.push(Feature {
            title: "Standalone tasks".to_string(),
            done,
            total,
            grouped: false,
        });
        for &i in &ungrouped {
            tasks.push(all[i].clone());
            feature_of.push(fi);
        }
    }

    Ok(BoardState {
        project,
        tasks,
        feature_of,
        features,
        selected: 0,
        scroll: 0,
    })
}

/// Sort key for a feature: (has any pending task, best pending urgency).
fn feature_sort_key(nodes: &[usize], all: &[Task]) -> (i32, f64) {
    let mut best = f64::MIN;
    let mut any_pending = 0;
    for &i in nodes {
        if all[i].status == Status::Pending {
            any_pending = 1;
            best = best.max(all[i].urgency);
        }
    }
    (any_pending, if any_pending == 1 { best } else { 0.0 })
}

/// Kahn topological sort over the component so blockers come before dependents.
/// Ties are broken by original position (urgency order) for stable output.
fn topo_order(
    nodes: &[usize],
    dependents: &HashMap<usize, Vec<usize>>,
    indeg: &[usize],
) -> Vec<usize> {
    use std::collections::HashSet;
    let set: HashSet<usize> = nodes.iter().copied().collect();
    let mut remaining: HashMap<usize, usize> = nodes.iter().map(|&i| (i, indeg[i])).collect();
    // Min-heap on position => earliest/most-urgent ready node first.
    let mut ready: BinaryHeap<Reverse<usize>> = remaining
        .iter()
        .filter(|&(_, &d)| d == 0)
        .map(|(&i, _)| Reverse(i))
        .collect();

    let mut out = Vec::with_capacity(nodes.len());
    while let Some(Reverse(i)) = ready.pop() {
        out.push(i);
        if let Some(deps) = dependents.get(&i) {
            for &j in deps {
                if !set.contains(&j) {
                    continue;
                }
                if let Some(d) = remaining.get_mut(&j) {
                    *d -= 1;
                    if *d == 0 {
                        ready.push(Reverse(j));
                    }
                }
            }
        }
    }
    // Any nodes left (shouldn't happen — graph is acyclic) appended in position order.
    if out.len() < nodes.len() {
        let mut leftover: Vec<usize> = nodes.iter().copied().filter(|i| !out.contains(i)).collect();
        leftover.sort_unstable();
        out.extend(leftover);
    }
    out
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]]; // path halving
        x = parent[x];
    }
    x
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        parent[ra] = rb;
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

fn board_loop<B: Backend>(terminal: &mut Terminal<B>, st: &mut BoardState) -> Result<BoardAction> {
    loop {
        // Keep the selected row inside the viewport (content height = total - borders - footer).
        let size = terminal.size()?;
        let viewport = size.height.saturating_sub(3);
        let (lines, task_line) = build_lines(st);
        if let Some(&line) = task_line.get(st.selected) {
            if line < st.scroll {
                st.scroll = line;
            } else if viewport > 0 && line >= st.scroll + viewport {
                st.scroll = line + 1 - viewport;
            }
        }

        terminal.draw(|f| render(f, st, &lines))?;

        if !event::poll(std::time::Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(BoardAction::Quit),
            KeyCode::Down | KeyCode::Char('j') => {
                if !st.tasks.is_empty() {
                    st.selected = (st.selected + 1).min(st.tasks.len() - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                st.selected = st.selected.saturating_sub(1);
            }
            KeyCode::PageDown => st.scroll = st.scroll.saturating_add(10),
            KeyCode::PageUp => st.scroll = st.scroll.saturating_sub(10),
            KeyCode::Enter => {
                if let Some(task) = st.tasks.get(st.selected) {
                    return Ok(BoardAction::OpenTask(task.uuid.to_string()));
                }
            }
            _ => {}
        }
    }
}

/// Build the rendered lines and a map from task index -> its line number, so the
/// scroll math and the renderer agree on layout.
fn build_lines(st: &BoardState) -> (Vec<Line<'static>>, Vec<u16>) {
    let mut lines: Vec<Line> = Vec::new();
    let mut task_line: Vec<u16> = vec![0; st.tasks.len()];
    let mut prev_feature: Option<usize> = None;

    for (idx, task) in st.tasks.iter().enumerate() {
        let fi = st.feature_of[idx];
        if prev_feature != Some(fi) {
            if prev_feature.is_some() {
                lines.push(Line::from(""));
            }
            let feat = &st.features[fi];
            lines.push(feature_header(feat));
            prev_feature = Some(fi);
        }

        task_line[idx] = lines.len() as u16;
        let is_sel = idx == st.selected;
        let grouped = st.features[fi].grouped;
        lines.push(task_line_for(task, is_sel, grouped));
    }
    (lines, task_line)
}

fn feature_header(feat: &Feature) -> Line<'static> {
    let complete = feat.total > 0 && feat.done == feat.total;
    let icon = if !feat.grouped {
        "•"
    } else if complete {
        "✓"
    } else {
        "▸"
    };
    let title_color = if complete {
        Color::Green
    } else if feat.grouped {
        Color::Cyan
    } else {
        Color::Gray
    };
    Line::from(vec![
        Span::styled(
            format!(" {icon} {}  ", feat.title),
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{}/{} done", feat.done, feat.total),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

fn task_line_for(task: &Task, is_sel: bool, grouped: bool) -> Line<'static> {
    let bg = if is_sel { Color::Blue } else { Color::Reset };
    let prefix = if is_sel { " ▶ " } else { "   " };
    // Chain connector for tasks that belong to a feature.
    let connector = if grouped { "└ " } else { "" };
    let id_str = task
        .id
        .map(|i| format!("{i:>3}"))
        .unwrap_or_else(|| "  -".to_string());

    if task.status == Status::Completed {
        let meta = Style::default()
            .fg(if is_sel {
                Color::White
            } else {
                Color::DarkGray
            })
            .bg(bg);
        let done_style = Style::default()
            .fg(if is_sel {
                Color::White
            } else {
                Color::DarkGray
            })
            .bg(bg)
            .add_modifier(Modifier::CROSSED_OUT);
        Line::from(vec![
            Span::styled(format!("{prefix}{connector}"), meta),
            Span::styled(format!("{id_str}  "), meta),
            Span::styled(task.description.clone(), done_style),
        ])
    } else {
        let pri_str = task.priority.as_ref().map(|p| p.label()).unwrap_or("-");
        let pri_color = match &task.priority {
            Some(Priority::H) => Color::Red,
            Some(Priority::M) => Color::Yellow,
            Some(Priority::L) => Color::Green,
            None => Color::DarkGray,
        };
        let (meta_style, id_style, pri_style, desc_style) = if is_sel {
            let s = Style::default().fg(Color::White).bg(bg);
            (s, s, s, s.add_modifier(Modifier::BOLD))
        } else {
            (
                Style::default().fg(Color::Gray),
                Style::default().fg(Color::Cyan),
                Style::default().fg(pri_color),
                Style::default(),
            )
        };
        Line::from(vec![
            Span::styled(format!("{prefix}{connector}"), meta_style),
            Span::styled(format!("{id_str}  "), id_style),
            Span::styled(format!("{pri_str:<4}  "), pri_style),
            Span::styled(task.description.clone(), desc_style),
        ])
    }
}

fn render(f: &mut Frame, st: &BoardState, lines: &[Line]) {
    let area = f.area();
    let pending = st
        .tasks
        .iter()
        .filter(|t| t.status == Status::Pending)
        .count();
    let done = st.tasks.len() - pending;
    let feature_count = st.features.iter().filter(|f| f.grouped).count();
    let title = format!(
        " {} · {} feature{} · {} pending, {} done ",
        st.project,
        feature_count,
        if feature_count == 1 { "" } else { "s" },
        pending,
        done
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let para = Paragraph::new(lines.to_vec())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false })
        .scroll((st.scroll, 0));
    f.render_widget(para, chunks[0]);

    let footer = Paragraph::new(Line::from(Span::styled(
        " j/k navigate  Enter open  PgDn/PgUp scroll  q quit",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(footer, chunks[1]);
}
