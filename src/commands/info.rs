use anyhow::Result;
use chrono::Local;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use rusqlite::Connection;

use crate::db;
use crate::model::{format_duration, Priority, Task};
use crate::tui;

struct Detail {
    task: Task,
    blocked_by: Vec<String>,
    blocking: Vec<String>,
    files: Vec<String>,
}

pub fn run(conn: &Connection, id_or_uuid: &str) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;

    let resolve_ids = |uuids: Vec<uuid::Uuid>| -> Vec<String> {
        uuids
            .iter()
            .filter_map(|u| {
                db::get_task_by_uuid_prefix(conn, &u.to_string()[..8])
                    .ok()
                    .flatten()
            })
            .map(|t| format!("[{}] {}", t.id.unwrap_or(0), t.description))
            .collect()
    };

    let detail = Detail {
        blocked_by: resolve_ids(db::get_blockers(conn, &task.uuid)?),
        blocking: resolve_ids(db::get_blocking(conn, &task.uuid)?),
        files: db::get_task_files(conn, &task.uuid)?,
        task,
    };

    // If not a TTY, fall back to plain text output
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        print_plain(&detail);
        return Ok(());
    }

    let mut terminal = tui::init_terminal()?;
    let mut scroll: u16 = 0;
    let result = (|| -> Result<()> {
        loop {
            terminal.draw(|f| render(f, &detail, scroll))?;
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter => break,
                    KeyCode::Down | KeyCode::Char('j') => scroll = scroll.saturating_add(1),
                    KeyCode::Up | KeyCode::Char('k') => scroll = scroll.saturating_sub(1),
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    tui::restore_terminal()?;
    result
}

fn render(f: &mut Frame, d: &Detail, scroll: u16) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let t = &d.task;
    let active = t.is_active();
    let title = format!(
        " Task {}{} ",
        t.id.map(|i| i.to_string()).unwrap_or_else(|| "-".into()),
        if active { "  ● ACTIVE" } else { "" }
    );

    let mut lines: Vec<Line> = vec![];

    lines.push(field("Description", &t.description));
    lines.push(field("Project", &t.project));
    lines.push(field("Status", &t.status.to_string()));

    let pri = match &t.priority {
        Some(Priority::H) => Span::styled("High", Style::default().fg(Color::Red)),
        Some(Priority::M) => Span::styled("Medium", Style::default().fg(Color::Yellow)),
        Some(Priority::L) => Span::styled("Low", Style::default().fg(Color::Green)),
        None => Span::styled("-", Style::default().fg(Color::DarkGray)),
    };
    lines.push(Line::from(vec![key_span("Priority"), pri]));

    let due = t
        .due
        .map(|dd| dd.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "-".to_string());
    let due_span = if let Some(dd) = t.due {
        let days = (dd - chrono::Utc::now()).num_days();
        let color = if days < 0 {
            Color::Red
        } else if days <= 1 {
            Color::Yellow
        } else {
            Color::Reset
        };
        Span::styled(due, Style::default().fg(color))
    } else {
        Span::styled(due, Style::default().fg(Color::DarkGray))
    };
    lines.push(Line::from(vec![key_span("Due"), due_span]));

    lines.push(field(
        "Tags",
        &if t.tags.is_empty() {
            "-".to_string()
        } else {
            t.tags.join(", ")
        },
    ));

    // Time tracking
    let time_str = if active {
        format!(
            "{}  (running, this session {})",
            format_duration(t.total_time_spent()),
            format_duration(t.total_time_spent() - t.time_spent)
        )
    } else if t.time_spent > 0 {
        format_duration(t.time_spent)
    } else {
        "-".to_string()
    };
    lines.push(Line::from(vec![
        key_span("Time spent"),
        Span::styled(
            time_str,
            Style::default().fg(if active { Color::Green } else { Color::Reset }),
        ),
    ]));

    lines.push(field("Urgency", &format!("{:.1}", t.urgency)));
    lines.push(field(
        "Entered",
        &t.entry.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string(),
    ));
    lines.push(field(
        "Modified",
        &t.modified.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string(),
    ));
    lines.push(field("UUID", &t.uuid.to_string()));

    if !d.blocked_by.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Blocked by"));
        for b in &d.blocked_by {
            lines.push(Line::from(format!("  {b}")));
        }
    }
    if !d.blocking.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Blocking"));
        for b in &d.blocking {
            lines.push(Line::from(format!("  {b}")));
        }
    }
    if !d.files.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Relevant files"));
        for file in &d.files {
            lines.push(Line::from(vec![Span::styled(
                format!("  {file}"),
                Style::default().fg(Color::Cyan),
            )]));
        }
    }

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(para, chunks[0]);

    let footer = " ↑/↓ scroll  •  q/Esc close ";
    f.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::DarkGray)),
        chunks[1],
    );
}

fn key_span(k: &str) -> Span<'static> {
    Span::styled(
        format!("{:<14}", k),
        Style::default().fg(Color::DarkGray),
    )
}

fn field<'a>(k: &str, v: &str) -> Line<'a> {
    Line::from(vec![key_span(k), Span::raw(v.to_string())])
}

fn section(k: &str) -> Line<'static> {
    Line::from(Span::styled(
        k.to_string(),
        Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
    ))
}

fn print_plain(d: &Detail) {
    let t = &d.task;
    println!("Task {}", t.id.unwrap_or(0));
    println!();
    println!("{:<14}{}", "Description", t.description);
    println!("{:<14}{}", "Project", t.project);
    println!("{:<14}{}", "Status", t.status);
    println!(
        "{:<14}{}",
        "Priority",
        t.priority.as_ref().map(|p| p.label()).unwrap_or("-")
    );
    println!(
        "{:<14}{}",
        "Due",
        t.due
            .map(|dd| dd.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "{:<14}{}",
        "Tags",
        if t.tags.is_empty() { "-".to_string() } else { t.tags.join(", ") }
    );
    println!("{:<14}{}", "Time spent", format_duration(t.total_time_spent()));
    println!("{:<14}{:.1}", "Urgency", t.urgency);
    println!("{:<14}{}", "UUID", t.uuid);
    for b in &d.blocked_by {
        println!("{:<14}{}", "Blocked by", b);
    }
    for b in &d.blocking {
        println!("{:<14}{}", "Blocking", b);
    }
    for file in &d.files {
        println!("{:<14}{}", "File", file);
    }
}
