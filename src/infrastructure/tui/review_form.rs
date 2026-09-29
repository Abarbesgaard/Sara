use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use ratatui_textarea::TextArea;

use crate::infrastructure::model::Priority;
use crate::infrastructure::tui::fzf;
use crate::infrastructure::tui::keymap::{self, Action};

#[derive(Debug, Clone)]
pub struct FormInput {
    pub description: String,
    pub project: String,
    pub priority: Option<Priority>,
    pub due: String,
    pub tags: String,
    pub selected_deps: Vec<usize>,
    pub selected_files: Vec<String>,
}

pub struct FormContext {
    pub initial: FormInput,
    pub available_deps: Vec<(String, String)>,
    pub available_files: Vec<String>,
    pub suggested_dep_indices: Vec<usize>,
    pub suggested_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Focus {
    Description,
    Project,
    Priority,
    Due,
    Tags,
    Dependencies,
    Files,
    Submit,
    Cancel,
}

const ALL_FIELDS: &[Focus] = &[
    Focus::Description,
    Focus::Project,
    Focus::Priority,
    Focus::Due,
    Focus::Tags,
    Focus::Dependencies,
    Focus::Files,
    Focus::Submit,
    Focus::Cancel,
];

struct FormState<'a> {
    focus: Focus,
    desc_area: TextArea<'a>,
    project_area: TextArea<'a>,
    due_area: TextArea<'a>,
    tags_area: TextArea<'a>,
    priority: Option<Priority>,
    dep_state: ListState,
    file_state: ListState,
    selected_deps: Vec<bool>,
    selected_file_paths: std::collections::BTreeSet<String>,
    file_filter: String,
    ctx: FormContext,
    submitted: bool,
    cancelled: bool,
    due_error: bool,
    due_preset_idx: usize,
    fzf_available: bool,
    fzf_requested: bool,
    showing_help: bool,
}

struct FileRow {
    path: String,
    selected: bool,
    suggested: bool,
    add_custom: bool,
}

impl<'a> FormState<'a> {
    fn new(ctx: FormContext) -> Self {
        let mut desc_area = TextArea::default();
        desc_area.insert_str(&ctx.initial.description);

        let mut project_area = TextArea::default();
        project_area.insert_str(&ctx.initial.project);

        let mut due_area = TextArea::default();
        due_area.insert_str(&ctx.initial.due);

        let mut tags_area = TextArea::default();
        tags_area.insert_str(&ctx.initial.tags);

        let n_deps = ctx.available_deps.len();

        let mut selected_deps = vec![false; n_deps];
        for &i in &ctx.initial.selected_deps {
            if i < n_deps {
                selected_deps[i] = true;
            }
        }
        let selected_file_paths: std::collections::BTreeSet<String> =
            ctx.initial.selected_files.iter().cloned().collect();

        let mut dep_state = ListState::default();
        if n_deps > 0 {
            dep_state.select(Some(0));
        }
        let mut file_state = ListState::default();
        if !ctx.available_files.is_empty() || !selected_file_paths.is_empty() {
            file_state.select(Some(0));
        }

        FormState {
            focus: Focus::Description,
            desc_area,
            project_area,
            due_area,
            tags_area,
            priority: ctx.initial.priority.clone(),
            dep_state,
            file_state,
            selected_deps,
            selected_file_paths,
            file_filter: String::new(),
            ctx,
            submitted: false,
            cancelled: false,
            due_error: false,
            due_preset_idx: 0,
            fzf_available: false,
            fzf_requested: false,
            showing_help: false,
        }
    }

    fn fzf_candidates(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut out = vec![];
        for p in self
            .ctx
            .available_files
            .iter()
            .chain(self.selected_file_paths.iter())
        {
            if seen.insert(p.clone()) {
                out.push(p.clone());
            }
        }
        out
    }

    fn file_rows(&self) -> Vec<FileRow> {
        let q = self.file_filter.trim().to_lowercase();
        let suggested: std::collections::HashSet<&String> =
            self.ctx.suggested_files.iter().collect();

        let mut rows: Vec<FileRow> = vec![];
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for path in &self.ctx.available_files {
            if q.is_empty() || path.to_lowercase().contains(&q) {
                seen.insert(path.clone());
                rows.push(FileRow {
                    path: path.clone(),
                    selected: self.selected_file_paths.contains(path),
                    suggested: suggested.contains(path),
                    add_custom: false,
                });
            }
        }
        for path in &self.selected_file_paths {
            if seen.contains(path) {
                continue;
            }
            if q.is_empty() || path.to_lowercase().contains(&q) {
                seen.insert(path.clone());
                rows.push(FileRow {
                    path: path.clone(),
                    selected: true,
                    suggested: false,
                    add_custom: false,
                });
            }
        }
        let typed = self.file_filter.trim();
        if !typed.is_empty() && rows.is_empty() {
            rows.push(FileRow {
                path: typed.to_string(),
                selected: false,
                suggested: false,
                add_custom: true,
            });
        }
        rows
    }

    fn next_focus(&mut self) {
        let idx = ALL_FIELDS
            .iter()
            .position(|f| *f == self.focus)
            .unwrap_or(0);
        self.focus = ALL_FIELDS[(idx + 1) % ALL_FIELDS.len()];
    }

    fn prev_focus(&mut self) {
        let idx = ALL_FIELDS
            .iter()
            .position(|f| *f == self.focus)
            .unwrap_or(0);
        self.focus = ALL_FIELDS[(idx + ALL_FIELDS.len() - 1) % ALL_FIELDS.len()];
    }

    fn toggle_dep(&mut self) {
        if let Some(i) = self.dep_state.selected()
            && i < self.selected_deps.len()
        {
            self.selected_deps[i] = !self.selected_deps[i];
        }
    }

    fn toggle_file(&mut self) {
        let rows = self.file_rows();
        let Some(i) = self.file_state.selected() else {
            return;
        };
        let Some(row) = rows.get(i) else {
            return;
        };
        if row.add_custom {
            self.selected_file_paths.insert(row.path.clone());
            self.file_filter.clear();
            self.file_state.select(Some(0));
        } else if self.selected_file_paths.contains(&row.path) {
            self.selected_file_paths.remove(&row.path);
        } else {
            self.selected_file_paths.insert(row.path.clone());
        }
    }

    fn clamp_file_selection(&mut self) {
        let n = self.file_rows().len();
        if n == 0 {
            self.file_state.select(None);
        } else {
            let cur = self.file_state.selected().unwrap_or(0);
            self.file_state.select(Some(cur.min(n - 1)));
        }
    }

    fn cycle_priority(&mut self, forward: bool) {
        self.priority = match (&self.priority, forward) {
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

    fn validate_due(&mut self) {
        let text = self.due_area.lines().join("");
        self.due_error = !crate::infrastructure::util::dates::is_valid_due(&text);
    }

    fn set_due_text(&mut self, value: &str) {
        let mut ta = TextArea::default();
        ta.insert_str(value);
        self.due_area = ta;
        self.validate_due();
    }

    fn cycle_due(&mut self, forward: bool) {
        let presets = crate::infrastructure::util::dates::DUE_PRESETS;
        let current = self.due_area.lines().join("");
        let cur_idx = presets
            .iter()
            .position(|p| *p == current.trim())
            .unwrap_or(0);
        let len = presets.len();
        let next = if forward {
            (cur_idx + 1) % len
        } else {
            (cur_idx + len - 1) % len
        };
        self.due_preset_idx = next;
        let value = presets[next].to_string();
        self.set_due_text(&value);
    }

    fn can_submit(&self) -> bool {
        !self.desc_area.lines().join("").trim().is_empty() && !self.due_error
    }

    fn input_to_focused_text_field(&mut self, key: crossterm::event::KeyEvent) -> bool {
        match self.focus {
            Focus::Description => {
                self.desc_area.input(key);
            }
            Focus::Project => {
                self.project_area.input(key);
            }
            Focus::Due => {
                self.due_area.input(key);
                self.validate_due();
            }
            Focus::Tags => {
                self.tags_area.input(key);
            }
            _ => return false,
        }
        true
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if self.showing_help {
            self.showing_help = false;
            return;
        }
        if key.code == KeyCode::Char('?')
            && !matches!(
                self.focus,
                Focus::Description | Focus::Project | Focus::Due | Focus::Tags
            )
        {
            self.showing_help = true;
            return;
        }
        match keymap::control_action(key) {
            Some(Action::Cancel) => {
                self.cancelled = true;
                return;
            }
            Some(Action::Save) => {
                if self.can_submit() {
                    self.submitted = true;
                }
                return;
            }
            Some(Action::NextFocus) => {
                self.next_focus();
                return;
            }
            Some(Action::PrevFocus) => {
                self.prev_focus();
                return;
            }
            _ => {}
        }
        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => match self.focus {
                Focus::Submit => {
                    if self.can_submit() {
                        self.submitted = true;
                    }
                }
                Focus::Cancel => {
                    self.cancelled = true;
                }
                Focus::Dependencies => self.toggle_dep(),
                Focus::Files => {
                    if self.fzf_available {
                        self.fzf_requested = true;
                    } else {
                        self.toggle_file();
                    }
                }
                _ => self.next_focus(),
            },
            (KeyCode::Char(' '), _) => match self.focus {
                Focus::Dependencies => self.toggle_dep(),
                Focus::Files => self.toggle_file(),
                _ => {
                    self.input_to_focused_text_field(key);
                }
            },
            (KeyCode::Left, _) if self.focus == Focus::Priority => {
                self.cycle_priority(false);
            }
            (KeyCode::Right, _) if self.focus == Focus::Priority => {
                self.cycle_priority(true);
            }
            (KeyCode::Left, _) if self.focus == Focus::Due => {
                self.cycle_due(false);
            }
            (KeyCode::Right, _) if self.focus == Focus::Due => {
                self.cycle_due(true);
            }
            (KeyCode::Up, _) => match self.focus {
                Focus::Dependencies => {
                    let len = self.ctx.available_deps.len();
                    let cur = self.dep_state.selected().unwrap_or(0);
                    if len > 0 && cur > 0 {
                        self.dep_state.select(Some(cur - 1));
                    } else {
                        self.prev_focus();
                    }
                }
                Focus::Files => {
                    let len = self.file_rows().len();
                    let cur = self.file_state.selected().unwrap_or(0);
                    if len > 0 && cur > 0 {
                        self.file_state.select(Some(cur - 1));
                    } else {
                        self.prev_focus();
                    }
                }
                _ => self.prev_focus(),
            },
            (KeyCode::Down, _) => match self.focus {
                Focus::Dependencies => {
                    let len = self.ctx.available_deps.len();
                    let cur = self.dep_state.selected().unwrap_or(0);
                    if len > 0 && cur + 1 < len {
                        self.dep_state.select(Some(cur + 1));
                    } else {
                        self.next_focus();
                    }
                }
                Focus::Files => {
                    let len = self.file_rows().len();
                    let cur = self.file_state.selected().unwrap_or(0);
                    if len > 0 && cur + 1 < len {
                        self.file_state.select(Some(cur + 1));
                    } else {
                        self.next_focus();
                    }
                }
                _ => self.next_focus(),
            },
            (KeyCode::Backspace, _) if self.focus == Focus::Files => {
                self.file_filter.pop();
                self.file_state.select(Some(0));
                self.clamp_file_selection();
            }
            _ => match self.focus {
                Focus::Files => {
                    if let KeyCode::Char(c) = key.code {
                        self.file_filter.push(c);
                        self.file_state.select(Some(0));
                        self.clamp_file_selection();
                    }
                }
                _ => {
                    self.input_to_focused_text_field(key);
                }
            },
        }
    }

    fn collect_result(&self) -> FormInput {
        let dep_indices = self
            .selected_deps
            .iter()
            .enumerate()
            .filter(|(_, v)| **v)
            .map(|(i, _)| i)
            .collect();
        let file_paths: Vec<String> = self.selected_file_paths.iter().cloned().collect();
        FormInput {
            description: self.desc_area.lines().join(""),
            project: self.project_area.lines().join(""),
            priority: self.priority.clone(),
            due: self.due_area.lines().join(""),
            tags: self.tags_area.lines().join(""),
            selected_deps: dep_indices,
            selected_files: file_paths,
        }
    }
}

pub fn run_form<B: Backend<Error: Send + Sync + 'static>>(
    terminal: &mut Terminal<B>,
    ctx: FormContext,
) -> Result<Option<FormInput>> {
    let mut state = FormState::new(ctx);
    state.fzf_available = fzf::fzf_available();

    loop {
        terminal.draw(|f| render(f, &mut state))?;

        if !event::poll(std::time::Duration::from_millis(100))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Release {
                continue;
            }
            state.handle_key(key);
        }

        if state.fzf_requested {
            state.fzf_requested = false;
            let candidates = state.fzf_candidates();
            crate::infrastructure::tui::suspend()?;
            let picked = fzf::run_fzf(&candidates, &state.file_filter);
            crate::infrastructure::tui::resume()?;
            terminal.clear()?;
            if let Some(paths) = picked {
                for p in paths {
                    state.selected_file_paths.insert(p);
                }
                state.file_filter.clear();
                state.clamp_file_selection();
            }
        }

        if state.submitted {
            return Ok(Some(state.collect_result()));
        }
        if state.cancelled {
            return Ok(None);
        }
    }
}

fn render(f: &mut Frame, state: &mut FormState) {
    let area = f.area();
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" sara — Review Task "),
        area,
    );

    let inner = shrink(area, 1);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let fields_area = chunks[0];
    let footer = chunks[1];

    render_fields(f, state, fields_area);
    render_footer(f, state, footer);

    if state.showing_help {
        crate::infrastructure::tui::render_help_overlay(f, "Review task", &help_bindings());
    }
}

fn help_bindings() -> Vec<(&'static str, &'static str)> {
    let mut v = keymap::help::CONTROL_ACTION_BINDINGS.to_vec();
    v.extend([
        (
            "↑ / ↓",
            "move within a field, or to the previous/next field",
        ),
        (
            "Enter",
            "toggle / open fzf / submit / cancel (depends on focus)",
        ),
        ("Space", "toggle a dependency or file"),
        ("← / →", "cycle priority or a due-date preset"),
        ("?", "toggle this help"),
    ]);
    v
}

fn shrink(r: Rect, n: u16) -> Rect {
    Rect {
        x: r.x + n,
        y: r.y + n,
        width: r.width.saturating_sub(n * 2),
        height: r.height.saturating_sub(n * 2),
    }
}

fn render_description(f: &mut Frame, state: &mut FormState, area: Rect) {
    let focused = state.focus == Focus::Description;
    let block = field_block("Description", focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    state.desc_area.set_block(Block::default());
    f.render_widget(&state.desc_area, inner);
}

fn render_project(f: &mut Frame, state: &mut FormState, area: Rect) {
    let focused = state.focus == Focus::Project;
    let block = field_block("Project", focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    state.project_area.set_block(Block::default());
    f.render_widget(&state.project_area, inner);
}

fn render_priority(f: &mut Frame, state: &mut FormState, area: Rect) {
    let focused = state.focus == Focus::Priority;
    let block = field_block("Priority  ←/→ to cycle", focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let label = match &state.priority {
        None => Span::styled("None", Style::default().fg(Color::DarkGray)),
        Some(Priority::L) => Span::styled("L  (Low)", Style::default().fg(Color::Green)),
        Some(Priority::M) => Span::styled("M  (Medium)", Style::default().fg(Color::Yellow)),
        Some(Priority::H) => Span::styled("H  (High)", Style::default().fg(Color::Red)),
    };
    f.render_widget(Paragraph::new(Line::from(label)), inner);
}

fn render_due(f: &mut Frame, state: &mut FormState, area: Rect) {
    let focused = state.focus == Focus::Due;
    let title = if state.due_error {
        "Due  ⚠ invalid date"
    } else {
        "Due  ←/→ presets, or type (2026-06-20, friday, +3d)"
    };
    let block = if state.due_error {
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Red))
    } else {
        field_block(title, focused)
    };
    let inner = block.inner(area);
    f.render_widget(block, area);
    state.due_area.set_block(Block::default());
    f.render_widget(&state.due_area, inner);
}

fn render_tags(f: &mut Frame, state: &mut FormState, area: Rect) {
    let focused = state.focus == Focus::Tags;
    let block = field_block("Tags  (comma-separated)", focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    state.tags_area.set_block(Block::default());
    f.render_widget(&state.tags_area, inner);
}

fn render_fields(f: &mut Frame, state: &mut FormState, area: Rect) {
    let heights = [3u16, 3, 3, 3, 3, 5, 7, 3];
    if area.height < 4 {
        return;
    }

    let constraints: Vec<Constraint> = heights.iter().map(|&h| Constraint::Length(h)).collect();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    render_description(f, state, rows[0]);
    render_project(f, state, rows[1]);
    render_priority(f, state, rows[2]);
    render_due(f, state, rows[3]);
    render_tags(f, state, rows[4]);
    render_dependencies(f, state, rows[5]);
    render_files(f, state, rows[6]);
    render_buttons(f, state, rows[7]);
}

fn render_dependencies(f: &mut Frame, state: &mut FormState, area: Rect) {
    let focused = state.focus == Focus::Dependencies;
    let block = field_block("Dependencies  (↑/↓ move, space toggle)", focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if state.ctx.available_deps.is_empty() {
        f.render_widget(
            Paragraph::new("No existing tasks").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
    } else {
        let items: Vec<ListItem> = state
            .ctx
            .available_deps
            .iter()
            .enumerate()
            .map(|(i, (id, desc))| {
                let check = if state.selected_deps[i] { "☑" } else { "☐" };
                let suggested = state.ctx.suggested_dep_indices.contains(&i);
                let style = if state.selected_deps[i] {
                    Style::default().fg(Color::Green)
                } else if suggested {
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{check} {id}  {desc}")).style(style)
            })
            .collect();
        let list = List::new(items).highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
        f.render_stateful_widget(list, inner, &mut state.dep_state);
    }
}

fn render_files(f: &mut Frame, state: &mut FormState, area: Rect) {
    let focused = state.focus == Focus::Files;
    let n_selected = state.selected_file_paths.len();
    let hint = if state.fzf_available {
        "Enter: fzf · space toggle · type to filter"
    } else {
        "type to filter · space toggle · Enter add typed"
    };
    let mut title = format!("Relevant Files  ({hint})");
    if n_selected > 0 {
        title = format!("Relevant Files  [{n_selected} selected]  ({hint})");
    }
    let block = field_block(&title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let show_filter = focused && !state.fzf_available;
    let (filter_area, list_area) = if show_filter {
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner);
        (Some(parts[0]), parts[1])
    } else {
        (None, inner)
    };
    if let Some(fa) = filter_area {
        f.render_widget(
            Paragraph::new(format!("🔍 {}", state.file_filter))
                .style(Style::default().fg(Color::Yellow)),
            fa,
        );
    }

    let file_rows = state.file_rows();
    if file_rows.is_empty() {
        let msg = if state.ctx.available_files.is_empty() {
            "No project files found — type a path, Enter to add"
        } else {
            "No matches"
        };
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(Color::DarkGray)),
            list_area,
        );
    } else {
        let items: Vec<ListItem> = file_rows
            .iter()
            .map(|r| {
                if r.add_custom {
                    return ListItem::new(format!("＋ add \"{}\"", r.path))
                        .style(Style::default().fg(Color::Magenta));
                }
                let check = if r.selected { "☑" } else { "☐" };
                let style = if r.selected {
                    Style::default().fg(Color::Green)
                } else if r.suggested {
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{check} {}", r.path)).style(style)
            })
            .collect();
        let list = List::new(items).highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
        f.render_stateful_widget(list, list_area, &mut state.file_state);
    }
}

fn render_buttons(f: &mut Frame, state: &mut FormState, area: Rect) {
    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let submit_style = if state.focus == Focus::Submit {
        Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else if state.can_submit() {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(
        Paragraph::new(" ✔  Save  (Ctrl+S)")
            .style(submit_style)
            .block(Block::default().borders(Borders::ALL)),
        halves[0],
    );

    let cancel_style = if state.focus == Focus::Cancel {
        Style::default()
            .bg(Color::Red)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    };
    f.render_widget(
        Paragraph::new(" ✖  Cancel  (Esc)")
            .style(cancel_style)
            .block(Block::default().borders(Borders::ALL)),
        halves[1],
    );
}

fn render_footer(f: &mut Frame, _state: &FormState, area: Rect) {
    let text = " Tab/Shift+Tab: move  •  ←/→: cycle priority  •  Space: toggle  •  Ctrl+S: save  •  ?: help  •  Esc: cancel ";
    f.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn field_block(title: &str, focused: bool) -> Block<'_> {
    if focused {
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title} "))
            .border_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
    } else {
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title} "))
            .border_style(Style::default().fg(Color::DarkGray))
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/infrastructure/tui/review_form.rs"]
mod tests;
