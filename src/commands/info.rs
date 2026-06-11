use anyhow::Result;
use chrono::{Local, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use rusqlite::Connection;
use tui_textarea::TextArea;

use crate::config::Config;
use crate::db;
use crate::model::{format_duration, Priority, Task};
use crate::tui;

struct Detail {
    task: Task,
    blocked_by: Vec<String>,
    blocking: Vec<String>,
    /// Files the user attached themselves.
    manual_files: Vec<String>,
    /// Files proposed by the LLM.
    suggested_files: Vec<String>,
    annotations: Vec<crate::db::Annotation>,
    history: Vec<crate::db::HistoryEntry>,
}

#[derive(Clone, Copy, PartialEq)]
enum EditField {
    Description,
    Project,
    Priority,
    Due,
    Tags,
}

const EDIT_FIELDS: [EditField; 5] = [
    EditField::Description,
    EditField::Project,
    EditField::Priority,
    EditField::Due,
    EditField::Tags,
];

impl EditField {
    fn label(&self) -> &'static str {
        match self {
            EditField::Description => "Description",
            EditField::Project => "Project",
            EditField::Priority => "Priority",
            EditField::Due => "Due",
            EditField::Tags => "Tags",
        }
    }
}

fn load_detail(conn: &Connection, task: Task) -> Result<Detail> {
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

    let sourced = db::get_task_files_sourced(conn, &task.uuid)?;
    let mut manual_files = vec![];
    let mut suggested_files = vec![];
    for (path, source) in sourced {
        if source == db::SOURCE_SUGGESTED {
            suggested_files.push(path);
        } else {
            manual_files.push(path);
        }
    }

    Ok(Detail {
        blocked_by: resolve_ids(db::get_blockers(conn, &task.uuid)?),
        blocking: resolve_ids(db::get_blocking(conn, &task.uuid)?),
        manual_files,
        suggested_files,
        annotations: db::get_annotations(conn, &task.uuid)?,
        history: db::get_history(conn, &task.uuid)?,
        task,
    })
}

pub fn run(conn: &Connection, cfg: &Config, id_or_uuid: &str) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    let detail = load_detail(conn, task)?;

    // If not a TTY, fall back to plain text output (read-only).
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        print_plain(&detail);
        return Ok(());
    }

    let mut terminal = tui::init_terminal()?;
    let result = edit_loop(&mut terminal, conn, cfg, detail);
    tui::restore_terminal()?;
    result.map(|_| ())
}

struct EditState {
    detail: Detail,
    selected: usize,
    editing: bool,
    editor: TextArea<'static>,
    due_error: bool,
    scroll: u16,
}

fn edit_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    conn: &Connection,
    cfg: &Config,
    detail: Detail,
) -> Result<()> {
    let mut st = EditState {
        detail,
        selected: 0,
        editing: false,
        editor: TextArea::default(),
        due_error: false,
        scroll: 0,
    };

    loop {
        terminal.draw(|f| render(f, &st))?;

        if !event::poll(std::time::Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }

        if st.editing {
            match key.code {
                KeyCode::Enter => {
                    let value = st.editor.lines().join("");
                    if EDIT_FIELDS[st.selected] == EditField::Due
                        && !value.trim().is_empty()
                        && !crate::dates::is_valid_due(&value)
                    {
                        st.due_error = true;
                        continue;
                    }
                    apply_field(&mut st.detail.task, EDIT_FIELDS[st.selected], &value, cfg);
                    save(conn, cfg, &mut st.detail)?;
                    st.editing = false;
                    st.due_error = false;
                }
                KeyCode::Esc => {
                    st.editing = false;
                    st.due_error = false;
                }
                _ => {
                    st.editor.input(key);
                    if EDIT_FIELDS[st.selected] == EditField::Due {
                        let v = st.editor.lines().join("");
                        st.due_error = !v.trim().is_empty() && !crate::dates::is_valid_due(&v);
                    }
                }
            }
        } else {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Down | KeyCode::Char('j') => {
                    st.selected = (st.selected + 1).min(EDIT_FIELDS.len() - 1);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    st.selected = st.selected.saturating_sub(1);
                }
                KeyCode::PageDown => st.scroll = st.scroll.saturating_add(5),
                KeyCode::PageUp => st.scroll = st.scroll.saturating_sub(5),
                KeyCode::Left if EDIT_FIELDS[st.selected] == EditField::Priority => {
                    cycle_priority(&mut st.detail.task, false);
                    save(conn, cfg, &mut st.detail)?;
                }
                KeyCode::Right if EDIT_FIELDS[st.selected] == EditField::Priority => {
                    cycle_priority(&mut st.detail.task, true);
                    save(conn, cfg, &mut st.detail)?;
                }
                KeyCode::Enter | KeyCode::Char('e') => match EDIT_FIELDS[st.selected] {
                    EditField::Priority => {
                        cycle_priority(&mut st.detail.task, true);
                        save(conn, cfg, &mut st.detail)?;
                    }
                    field => {
                        st.editor = editor_for(&st.detail.task, field);
                        st.editing = true;
                        st.due_error = false;
                    }
                },
                _ => {}
            }
        }
    }

    Ok(())
}

fn editor_for(task: &Task, field: EditField) -> TextArea<'static> {
    let value = current_value(task, field);
    let mut ta = TextArea::default();
    ta.insert_str(&value);
    ta
}

fn current_value(task: &Task, field: EditField) -> String {
    match field {
        EditField::Description => task.description.clone(),
        EditField::Project => task.project.clone(),
        EditField::Priority => task
            .priority
            .as_ref()
            .map(|p| p.label().to_string())
            .unwrap_or_default(),
        EditField::Due => task
            .due
            .map(|d| d.with_timezone(&Local).format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
        EditField::Tags => task.tags.join(", "),
    }
}

fn apply_field(task: &mut Task, field: EditField, value: &str, cfg: &Config) {
    match field {
        EditField::Description => {
            if !value.trim().is_empty() {
                task.description = value.trim().to_string();
            }
        }
        EditField::Project => {
            if !value.trim().is_empty() {
                task.project = value.trim().to_string();
            }
        }
        EditField::Due => {
            if value.trim().is_empty() {
                task.due = None;
            } else {
                task.due = crate::commands::add::parse_due(value, cfg);
            }
        }
        EditField::Tags => {
            task.tags = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        EditField::Priority => {}
    }
}

fn cycle_priority(task: &mut Task, forward: bool) {
    task.priority = match (&task.priority, forward) {
        (None, true) => Some(Priority::L),
        (Some(Priority::L), true) => Some(Priority::M),
        (Some(Priority::M), true) => Some(Priority::H),
        (Some(Priority::H), true) => None,
        (None, false) => Some(Priority::H),
        (Some(Priority::H), false) => Some(Priority::M),
        (Some(Priority::M), false) => Some(Priority::L),
        (Some(Priority::L), false) => None,
    };
}

fn save(conn: &Connection, cfg: &Config, detail: &mut Detail) -> Result<()> {
    let task = &mut detail.task;
    task.modified = Utc::now();
    task.urgency = db::compute_urgency(task, &cfg.urgency, false, 0);
    db::update_task(conn, task)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;
    // Pull back the authoritative urgency (refresh accounts for blocking).
    if let Some(t) = db::get_task_by_uuid_prefix(conn, &task.uuid.to_string()[..8])? {
        task.urgency = t.urgency;
    }
    detail.history = db::get_history(conn, &detail.task.uuid)?;
    Ok(())
}

fn render(f: &mut Frame, st: &EditState) {
    let area = f.area();

    let constraints = if st.editing {
        vec![Constraint::Min(1), Constraint::Length(3), Constraint::Length(1)]
    } else {
        vec![Constraint::Min(1), Constraint::Length(1)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let d = &st.detail;
    let t = &d.task;
    let active = t.is_active();
    let title = format!(
        " Task {}{} ",
        t.id.map(|i| i.to_string()).unwrap_or_else(|| "-".into()),
        if active { "  ● ACTIVE" } else { "" }
    );

    let mut lines: Vec<Line> = vec![];

    // ── Editable fields
    for (i, field) in EDIT_FIELDS.iter().enumerate() {
        let selected = !st.editing && i == st.selected;
        let editing_this = st.editing && i == st.selected;
        let value = if editing_this {
            "…(editing below)".to_string()
        } else {
            let v = current_value(t, *field);
            if v.is_empty() { "-".to_string() } else { v }
        };
        lines.push(editable_line(field.label(), &value, selected, *field, t));
    }

    // ── Read-only fields
    lines.push(field_line("Status", &t.status.to_string()));
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
    lines.push(field_line("Urgency", &format!("{:.1}", t.urgency)));
    lines.push(field_line(
        "Entered",
        &t.entry.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string(),
    ));
    lines.push(field_line(
        "Modified",
        &t.modified.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string(),
    ));
    lines.push(field_line("UUID", &t.uuid.to_string()));

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
    if !d.manual_files.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Relevant files"));
        for file in &d.manual_files {
            lines.push(Line::from(Span::styled(
                format!("  {file}"),
                Style::default().fg(Color::Cyan),
            )));
        }
    }
    if !d.suggested_files.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Possible relevant files (suggested by AI)"));
        for file in &d.suggested_files {
            lines.push(Line::from(Span::styled(
                format!("  {file}"),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )));
        }
    }
    if !d.annotations.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Annotations / comments"));
        for a in &d.annotations {
            let date = a.entry.with_timezone(&Local).format("%Y-%m-%d %H:%M");
            lines.push(Line::from(vec![
                Span::styled(format!("  [{}] ", a.id), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{date}  "), Style::default().fg(Color::DarkGray)),
                Span::raw(a.text.clone()),
            ]));
        }
    }
    if !d.history.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("History"));
        for h in &d.history {
            let date = h.changed_at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
            let mut spans = vec![
                Span::styled(format!("  {date}  "), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<11} ", h.field), Style::default().fg(Color::Cyan)),
            ];
            if h.field == "created" {
                spans.push(Span::raw(h.new_value.clone().unwrap_or_default()));
            } else {
                spans.push(Span::styled(
                    h.old_value.clone().unwrap_or_else(|| "—".into()),
                    Style::default().fg(Color::DarkGray),
                ));
                spans.push(Span::styled(" → ", Style::default().fg(Color::DarkGray)));
                spans.push(Span::raw(h.new_value.clone().unwrap_or_else(|| "—".into())));
            }
            lines.push(Line::from(spans));
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
        .scroll((st.scroll, 0));
    f.render_widget(para, chunks[0]);

    // ── Edit bar
    if st.editing {
        let field = EDIT_FIELDS[st.selected];
        let (title, border) = if st.due_error {
            (
                format!(" Editing {} — invalid date ", field.label()),
                Color::Red,
            )
        } else {
            (
                format!(" Editing {}  (Enter confirm · Esc cancel) ", field.label()),
                Color::Yellow,
            )
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(border));
        let inner = block.inner(chunks[1]);
        f.render_widget(block, chunks[1]);
        f.render_widget(&st.editor, inner);
    }

    let footer = if st.editing {
        " type to edit  •  Enter confirm  •  Esc cancel ".to_string()
    } else {
        " ↑/↓ select field  •  Enter/e edit  •  ←/→ change priority  •  PgUp/PgDn scroll  •  q close ".to_string()
    };
    let footer_idx = chunks.len() - 1;
    f.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::DarkGray)),
        chunks[footer_idx],
    );
}

fn editable_line<'a>(
    k: &str,
    v: &str,
    selected: bool,
    field: EditField,
    task: &Task,
) -> Line<'a> {
    let marker = if selected { "› " } else { "  " };
    let key_style = if selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    // Priority gets a colored value.
    let value_span = if field == EditField::Priority {
        match &task.priority {
            Some(Priority::H) => Span::styled("High", Style::default().fg(Color::Red)),
            Some(Priority::M) => Span::styled("Medium", Style::default().fg(Color::Yellow)),
            Some(Priority::L) => Span::styled("Low", Style::default().fg(Color::Green)),
            None => Span::styled("-", Style::default().fg(Color::DarkGray)),
        }
    } else if field == EditField::Due {
        due_value_span(task, v)
    } else {
        Span::raw(v.to_string())
    };

    Line::from(vec![
        Span::styled(marker.to_string(), key_style),
        Span::styled(format!("{:<12}", k), key_style),
        value_span,
    ])
}

fn due_value_span<'a>(task: &Task, fallback: &str) -> Span<'a> {
    if let Some(dd) = task.due {
        let days = (dd - Utc::now()).num_days();
        let color = if days < 0 {
            Color::Red
        } else if days <= 1 {
            Color::Yellow
        } else {
            Color::Reset
        };
        Span::styled(
            dd.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string(),
            Style::default().fg(color),
        )
    } else {
        Span::styled(fallback.to_string(), Style::default().fg(Color::DarkGray))
    }
}

fn key_span(k: &str) -> Span<'static> {
    Span::styled(format!("  {:<12}", k), Style::default().fg(Color::DarkGray))
}

fn field_line<'a>(k: &str, v: &str) -> Line<'a> {
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
    for file in &d.manual_files {
        println!("{:<14}{}", "File", file);
    }
    for file in &d.suggested_files {
        println!("{:<14}{}", "Possible file", file);
    }
    for a in &d.annotations {
        let date = a.entry.with_timezone(&Local).format("%Y-%m-%d %H:%M");
        println!("{:<14}[{}] {} {}", "Annotation", a.id, date, a.text);
    }
    for h in &d.history {
        let date = h.changed_at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
        let change = if h.field == "created" {
            h.new_value.clone().unwrap_or_default()
        } else {
            format!(
                "{}: {} -> {}",
                h.field,
                h.old_value.as_deref().unwrap_or("-"),
                h.new_value.as_deref().unwrap_or("-"),
            )
        };
        println!("{:<14}{} {}", "History", date, change);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> Task {
        Task::new("original".into(), "tk".into())
    }

    #[test]
    fn editing_description_updates_value() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Description, "new description", &cfg);
        assert_eq!(t.description, "new description");
    }

    #[test]
    fn empty_description_is_ignored() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Description, "   ", &cfg);
        assert_eq!(t.description, "original");
    }

    #[test]
    fn editing_tags_splits_and_trims() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Tags, " rust , cli ,", &cfg);
        assert_eq!(t.tags, vec!["rust".to_string(), "cli".to_string()]);
    }

    #[test]
    fn editing_due_empty_clears_it() {
        let mut t = task();
        let cfg = Config::default();
        t.due = Some(Utc::now());
        apply_field(&mut t, EditField::Due, "", &cfg);
        assert!(t.due.is_none());
    }

    #[test]
    fn editing_due_parses_relative() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Due, "+3d", &cfg);
        assert!(t.due.is_some());
    }

    #[test]
    fn priority_cycles_forward_and_back() {
        let mut t = task();
        assert!(t.priority.is_none());
        cycle_priority(&mut t, true);
        assert_eq!(t.priority, Some(Priority::L));
        cycle_priority(&mut t, true);
        assert_eq!(t.priority, Some(Priority::M));
        cycle_priority(&mut t, true);
        assert_eq!(t.priority, Some(Priority::H));
        cycle_priority(&mut t, true);
        assert!(t.priority.is_none());
        cycle_priority(&mut t, false);
        assert_eq!(t.priority, Some(Priority::H));
    }

    #[test]
    fn current_value_round_trips_with_apply() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Project, "myproj", &cfg);
        assert_eq!(current_value(&t, EditField::Project), "myproj");
        apply_field(&mut t, EditField::Tags, "a, b", &cfg);
        assert_eq!(current_value(&t, EditField::Tags), "a, b");
    }
}
