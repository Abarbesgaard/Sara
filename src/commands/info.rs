use anyhow::Result;
use chrono::{Local, Utc};
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
    links: Vec<crate::db::Link>,
    annotations: Vec<crate::db::Annotation>,
    history: Vec<crate::db::HistoryEntry>,
    /// Absolute project root, used to open relative file paths.
    project_root: Option<std::path::PathBuf>,
    /// Persisted branch snapshot (set via `tk addbranch`, populated on `tk stop`).
    branch: Option<crate::db::BranchRecord>,
    /// Tasks in the same project whose snapshot files overlap with this task's.
    overlaps: Vec<BranchOverlap>,
}

struct BranchOverlap {
    id: i64,
    description: String,
    branch: String,
    shared_files: Vec<String>,
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

/// Something the cursor can land on in the detail view.
#[derive(Clone, PartialEq)]
enum Focusable {
    Field(EditField),
    File(String),
    Link(usize),
}

/// Ordered list of focusable items: editable fields, then links, then files.
/// (Matches the on-screen order so arrow-key navigation feels natural.)
fn focusables(d: &Detail) -> Vec<Focusable> {
    let mut v: Vec<Focusable> = EDIT_FIELDS.iter().map(|f| Focusable::Field(*f)).collect();
    for i in 0..d.links.len() {
        v.push(Focusable::Link(i));
    }
    for f in d.manual_files.iter().chain(d.suggested_files.iter()) {
        v.push(Focusable::File(f.clone()));
    }
    v
}

/// Open a URL in the OS default browser (non-blocking). Adds a scheme for
/// bare `www.` style links.
fn open_url(raw: &str) {
    let url = if raw.starts_with("www.") {
        format!("https://{raw}")
    } else {
        raw.to_string()
    };
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd)
        .arg(&url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Pick the user's terminal editor: $VISUAL, then $EDITOR, then the first of
/// nvim/vim/nano that exists on PATH.
fn editor_command() -> String {
    if let Ok(v) = std::env::var("VISUAL") {
        if !v.trim().is_empty() {
            return v;
        }
    }
    if let Ok(v) = std::env::var("EDITOR") {
        if !v.trim().is_empty() {
            return v;
        }
    }
    for candidate in ["nvim", "vim", "nano", "vi"] {
        if std::process::Command::new("which")
            .arg(candidate)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return candidate.to_string();
        }
    }
    "vi".to_string()
}

/// Launch the editor on `path`, inheriting stdio so it takes over the terminal.
/// The caller is responsible for suspending/resuming the TUI around this.
fn open_in_editor(path: &std::path::Path) -> std::io::Result<()> {
    // $EDITOR may contain args (e.g. "code -w"); split on whitespace.
    let editor = editor_command();
    let mut parts = editor.split_whitespace();
    let bin = parts.next().unwrap_or("vi");
    let mut cmd = std::process::Command::new(bin);
    cmd.args(parts).arg(path);
    cmd.status().map(|_| ())
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

    let project_root = db::get_project(conn, &task.project)?
        .and_then(|p| p.path)
        .map(std::path::PathBuf::from);

    // Branch snapshot and overlap detection (pure stored-data, no live git).
    let branch = db::get_task_branch(conn, &task.uuid);
    let overlaps = compute_overlaps(conn, &task, &branch);

    Ok(Detail {
        blocked_by: resolve_ids(db::get_blockers(conn, &task.uuid)?),
        blocking: resolve_ids(db::get_blocking(conn, &task.uuid)?),
        manual_files,
        suggested_files,
        links: db::get_links(conn, &task.uuid)?,
        annotations: db::get_annotations(conn, &task.uuid)?,
        history: db::get_history(conn, &task.uuid)?,
        project_root,
        branch,
        overlaps,
        task,
    })
}

fn compute_overlaps(
    conn: &Connection,
    task: &Task,
    branch_rec: &Option<db::BranchRecord>,
) -> Vec<BranchOverlap> {
    let my_files: std::collections::HashSet<String> = branch_rec
        .as_ref()
        .and_then(|b| b.files.as_ref())
        .map(|fs| fs.iter().cloned().collect())
        .unwrap_or_default();

    if my_files.is_empty() {
        return vec![];
    }

    let others = db::branched_pending_in_project(conn, &task.project, &task.uuid)
        .unwrap_or_default();

    let mut result = vec![];
    for (id, desc, other_rec) in others {
        let other_files: std::collections::HashSet<String> = other_rec
            .files
            .as_ref()
            .map(|fs| fs.iter().cloned().collect())
            .unwrap_or_default();
        let mut shared: Vec<String> = my_files.intersection(&other_files).cloned().collect();
        if !shared.is_empty() {
            shared.sort();
            result.push(BranchOverlap {
                id,
                description: desc,
                branch: other_rec.branch,
                shared_files: shared,
            });
        }
    }
    result
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

        let items = focusables(&st.detail);
        // Keep the cursor in range (links/files can disappear after a reload).
        if !items.is_empty() && st.selected >= items.len() {
            st.selected = items.len() - 1;
        }
        let current = items.get(st.selected).cloned();
        let current_field = match &current {
            Some(Focusable::Field(f)) => Some(*f),
            _ => None,
        };

        if st.editing {
            let field = current_field.unwrap_or(EditField::Description);
            match key.code {
                KeyCode::Enter => {
                    let value = st.editor.lines().join("");
                    if field == EditField::Due
                        && !value.trim().is_empty()
                        && !crate::dates::is_valid_due(&value)
                    {
                        st.due_error = true;
                        continue;
                    }
                    apply_field(&mut st.detail.task, field, &value, cfg);
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
                    if field == EditField::Due {
                        let v = st.editor.lines().join("");
                        st.due_error = !v.trim().is_empty() && !crate::dates::is_valid_due(&v);
                    }
                }
            }
        } else {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Down | KeyCode::Char('j') => {
                    if !items.is_empty() {
                        st.selected = (st.selected + 1).min(items.len() - 1);
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    st.selected = st.selected.saturating_sub(1);
                }
                KeyCode::PageDown => st.scroll = st.scroll.saturating_add(5),
                KeyCode::PageUp => st.scroll = st.scroll.saturating_sub(5),
                KeyCode::Left if current_field == Some(EditField::Priority) => {
                    cycle_priority(&mut st.detail.task, false);
                    save(conn, cfg, &mut st.detail)?;
                }
                KeyCode::Right if current_field == Some(EditField::Priority) => {
                    cycle_priority(&mut st.detail.task, true);
                    save(conn, cfg, &mut st.detail)?;
                }
                KeyCode::Enter | KeyCode::Char('e') => match current {
                    Some(Focusable::Field(EditField::Priority)) => {
                        cycle_priority(&mut st.detail.task, true);
                        save(conn, cfg, &mut st.detail)?;
                    }
                    Some(Focusable::Field(field)) => {
                        st.editor = editor_for(&st.detail.task, field);
                        st.editing = true;
                        st.due_error = false;
                    }
                    Some(Focusable::Link(i)) => {
                        if let Some(link) = st.detail.links.get(i) {
                            open_url(&link.url);
                        }
                    }
                    Some(Focusable::File(path)) => {
                        if db::is_url(&path) {
                            // URL stored as a file (legacy attach) -> browser.
                            open_url(&path);
                        } else {
                            // Real file -> open in the user's editor. Hand the
                            // terminal back while the editor runs.
                            let target = st
                                .detail
                                .project_root
                                .as_ref()
                                .map(|r| r.join(&path))
                                .unwrap_or_else(|| std::path::PathBuf::from(&path));
                            tui::suspend()?;
                            let _ = open_in_editor(&target);
                            tui::resume()?;
                            terminal.clear()?;
                        }
                    }
                    None => {}
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
    // Reload branch / overlaps in case project changed.
    detail.branch = db::get_task_branch(conn, &detail.task.uuid);
    detail.overlaps = compute_overlaps(conn, &detail.task, &detail.branch);
    Ok(())
}

fn render(f: &mut Frame, st: &EditState) {
    let area = f.area();
    let d = &st.detail;

    let history_height: u16 = if d.history.is_empty() {
        0
    } else {
        (d.history.len() as u16 + 2).min(9) // border (2) + up to 7 entries
    };

    let constraints = if st.editing {
        if history_height > 0 {
            vec![Constraint::Min(1), Constraint::Length(history_height), Constraint::Length(3), Constraint::Length(1)]
        } else {
            vec![Constraint::Min(1), Constraint::Length(3), Constraint::Length(1)]
        }
    } else if history_height > 0 {
        vec![Constraint::Min(1), Constraint::Length(history_height), Constraint::Length(1)]
    } else {
        vec![Constraint::Min(1), Constraint::Length(1)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

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
    // Selected focusable (for highlighting files/links). Fields are handled
    // inline above via their index.
    let items = focusables(d);
    let sel = if st.editing { None } else { items.get(st.selected).cloned() };
    let file_selected = |path: &str| sel == Some(Focusable::File(path.to_string()));

    if !d.links.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Links  (Enter to open)"));
        for (i, link) in d.links.iter().enumerate() {
            let selected = sel == Some(Focusable::Link(i));
            let marker = if selected { "› " } else { "  " };
            let style = if selected {
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED)
            };
            let mut spans = vec![
                Span::styled(format!("{marker}[{}] ", link.id), Style::default().fg(Color::Gray)),
                Span::styled(link.display(), style),
            ];
            // Show the raw URL too when a label was derived/added.
            if link.display() != link.url {
                spans.push(Span::styled(
                    format!("  {}", link.url),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            lines.push(Line::from(spans));
        }
    }
    if !d.manual_files.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Relevant files"));
        for file in &d.manual_files {
            lines.push(nav_line(file, Color::Cyan, false, file_selected(file)));
        }
    }
    if !d.suggested_files.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Possible relevant files (suggested by AI)"));
        for file in &d.suggested_files {
            lines.push(nav_line(file, Color::Gray, true, file_selected(file)));
        }
    }
    if !d.annotations.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Comments"));
        for a in &d.annotations {
            let date = a.entry.with_timezone(&Local).format("%Y-%m-%d %H:%M");
            lines.push(Line::from(vec![
                Span::styled(format!("  [{}] ", a.id), Style::default().fg(Color::Gray)),
                Span::styled(format!("{date}  "), Style::default().fg(Color::Gray)),
                Span::raw(a.text.clone()),
            ]));
        }
    }
    // History is rendered in its own box at the bottom — not in the main lines.

    // Split the main content area horizontally when wide enough for the panel.
    let show_panel = chunks[0].width >= 96;
    let (left_area, panel_area) = if show_panel {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(50), Constraint::Length(42)])
            .split(chunks[0]);
        (cols[0], Some(cols[1]))
    } else {
        (chunks[0], None)
    };

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false })
        .scroll((st.scroll, 0));
    f.render_widget(para, left_area);

    // ── Git branch panel
    if let Some(panel) = panel_area {
        let git_lines = git_panel_lines(d);
        let git_para = Paragraph::new(git_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Git ")
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(git_para, panel);
    }

    // ── History box (pinned to bottom, above edit bar and footer)
    if history_height > 0 {
        let hist_chunk = chunks[1]; // always chunk[1] when history is shown
        let hist_lines = history_lines(&d.history);
        let hist_para = Paragraph::new(hist_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" History ")
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(hist_para, hist_chunk);
    }

    // ── Edit bar (chunk index depends on whether history box is present)
    if st.editing {
        let edit_chunk_idx = if history_height > 0 { 2 } else { 1 };
        let field = EDIT_FIELDS.get(st.selected).copied().unwrap_or(EditField::Description);
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
        let inner = block.inner(chunks[edit_chunk_idx]);
        f.render_widget(block, chunks[edit_chunk_idx]);
        f.render_widget(&st.editor, inner);
    }

    let footer = if st.editing {
        " type to edit  •  Enter confirm  •  Esc cancel ".to_string()
    } else {
        " ↑/↓ move  •  Enter edit/open  •  ←/→ priority  •  PgUp/PgDn scroll  •  q close ".to_string()
    };
    let footer_idx = chunks.len() - 1;
    f.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::Gray)),
        chunks[footer_idx],
    );
}

/// Build lines for the History box at the bottom of the detail view.
fn history_lines(history: &[crate::db::HistoryEntry]) -> Vec<Line<'static>> {
    let mut lines = vec![];
    for h in history.iter().rev() {
        let date = h.changed_at.with_timezone(&Local).format("%m-%d %H:%M").to_string();
        let label = if h.field == "annotation" { "comment" } else { &h.field };
        let mut spans = vec![
            Span::styled(format!("  {date}  "), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<11} ", label), Style::default().fg(Color::Cyan)),
        ];
        if h.field == "created" {
            spans.push(Span::raw(h.new_value.clone().unwrap_or_default()));
        } else if h.field == "annotation" || h.field == "link" {
            if let Some(text) = &h.new_value {
                spans.push(Span::styled("+ ", Style::default().fg(Color::Green)));
                spans.push(Span::raw(text.clone()));
            } else if let Some(text) = &h.old_value {
                spans.push(Span::styled("− ", Style::default().fg(Color::Red)));
                spans.push(Span::raw(text.clone()));
            }
        } else {
            spans.push(Span::styled(
                h.old_value.clone().unwrap_or_else(|| "—".into()),
                Style::default().fg(Color::Gray),
            ));
            spans.push(Span::styled(" → ", Style::default().fg(Color::DarkGray)));
            spans.push(Span::raw(h.new_value.clone().unwrap_or_else(|| "—".into())));
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// Build the content lines for the Git branch panel.
fn git_panel_lines(d: &Detail) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = vec![];

    let Some(rec) = &d.branch else {
        lines.push(Line::from(Span::styled(
            "  No branch tied.",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Run: tk <id> addbranch",
            Style::default().fg(Color::Gray),
        )));
        lines.push(Line::from(Span::styled(
            "  Then: tk stop <id> to snapshot.",
            Style::default().fg(Color::Gray),
        )));
        return lines;
    };

    // Branch name line
    lines.push(Line::from(vec![
        Span::styled("  Branch  ", Style::default().fg(Color::DarkGray)),
        Span::styled(rec.branch.clone(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ]));
    if let Some(base) = &rec.base {
        lines.push(Line::from(vec![
            Span::styled("  Base    ", Style::default().fg(Color::DarkGray)),
            Span::styled(base.clone(), Style::default().fg(Color::Gray)),
        ]));
    }
    if let Some(logged_at) = rec.logged_at {
        let ts = logged_at.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string();
        lines.push(Line::from(vec![
            Span::styled("  Logged  ", Style::default().fg(Color::DarkGray)),
            Span::styled(ts, Style::default().fg(Color::Gray)),
        ]));
    }
    lines.push(Line::from(""));

    match &rec.files {
        None => {
            lines.push(Line::from(Span::styled(
                "  No snapshot yet.",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                "  Run: tk stop <id>",
                Style::default().fg(Color::Gray),
            )));
        }
        Some(files) if files.is_empty() => {
            lines.push(Line::from(Span::styled(
                "  No changes vs base.",
                Style::default().fg(Color::Green),
            )));
        }
        Some(files) => {
            const MAX_FILES: usize = 20;
            lines.push(Line::from(Span::styled(
                format!("  {} file{} changed", files.len(), if files.len() == 1 { "" } else { "s" }),
                Style::default().fg(Color::Yellow),
            )));
            for f in files.iter().take(MAX_FILES) {
                // Show only filename for brevity; full path on hover isn't feasible in TUI
                let name = std::path::Path::new(f)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(f.as_str());
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(name.to_string(), Style::default().fg(Color::Cyan)),
                    if name != f.as_str() {
                        Span::styled(
                            format!("  {}", f),
                            Style::default().fg(Color::DarkGray),
                        )
                    } else {
                        Span::raw("")
                    },
                ]));
            }
            if files.len() > MAX_FILES {
                lines.push(Line::from(Span::styled(
                    format!("    +{} more", files.len() - MAX_FILES),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }

    // Overlap section
    if !d.overlaps.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  ⚠  Potential overlaps",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
        for ov in &d.overlaps {
            lines.push(Line::from(vec![
                Span::styled(format!("  [{:>2}] ", ov.id), Style::default().fg(Color::Gray)),
                Span::styled(
                    truncate_str(&ov.description, 20),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!(" ({})", ov.branch),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
            for sf in &ov.shared_files {
                let name = std::path::Path::new(sf)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(sf.as_str());
                lines.push(Line::from(Span::styled(
                    format!("    ↳ {name}"),
                    Style::default().fg(Color::Red),
                )));
            }
        }
    }

    lines
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max - 1).collect();
        format!("{t}…")
    }
}

/// A selectable file/link row with a `›` marker when focused.
fn nav_line<'a>(text: &str, color: Color, italic: bool, selected: bool) -> Line<'a> {
    let marker = if selected { "› " } else { "  " };
    let mut style = Style::default().fg(color);
    if italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if selected {
        style = style.add_modifier(Modifier::BOLD);
    }
    Line::from(vec![
        Span::styled(marker.to_string(), Style::default().fg(Color::Gray)),
        Span::styled(text.to_string(), style),
    ])
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
        Style::default().fg(Color::Gray)
    };

    // Priority gets a colored value.
    let value_span = if field == EditField::Priority {
        match &task.priority {
            Some(Priority::H) => Span::styled("High", Style::default().fg(Color::Red)),
            Some(Priority::M) => Span::styled("Medium", Style::default().fg(Color::Yellow)),
            Some(Priority::L) => Span::styled("Low", Style::default().fg(Color::Green)),
            None => Span::styled("-", Style::default().fg(Color::Gray)),
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
        Span::styled(fallback.to_string(), Style::default().fg(Color::Gray))
    }
}

fn key_span(k: &str) -> Span<'static> {
    Span::styled(format!("  {:<12}", k), Style::default().fg(Color::Gray))
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
    for link in &d.links {
        println!("{:<14}[{}] {}  {}", "Link", link.id, link.display(), link.url);
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
        } else if h.field == "annotation" {
            match (&h.new_value, &h.old_value) {
                (Some(text), _) => format!("comment added: {text}"),
                (None, Some(text)) => format!("comment removed: {text}"),
                _ => "comment".to_string(),
            }
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
