use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use tui_textarea::TextArea;

use crate::infrastructure::model::Priority;
use crate::infrastructure::tui::fzf;

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FormInput {
    pub description: String,
    pub project: String,
    pub priority: Option<Priority>,
    pub due: String,
    pub tags: String,
    pub selected_deps: Vec<usize>,
    /// Selected file paths (may include paths typed by the user that are not
    /// in `available_files`).
    pub selected_files: Vec<String>,
}

/// Input to the form: existing data + available choices.
pub struct FormContext {
    pub initial: FormInput,
    pub available_deps: Vec<(String, String)>, // (display_id, description)
    pub available_files: Vec<String>,
    /// Pre-selected dependency indices to highlight in the picker.
    pub suggested_dep_indices: Vec<usize>,
    /// File paths to highlight in the file picker.
    pub suggested_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
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

pub const ALL_FIELDS: &[Focus] = &[
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

pub struct FormState<'a> {
    pub focus: Focus,
    pub desc_area: TextArea<'a>,
    pub project_area: TextArea<'a>,
    pub due_area: TextArea<'a>,
    pub tags_area: TextArea<'a>,
    pub priority: Option<Priority>,
    pub dep_state: ListState,
    pub file_state: ListState,
    pub selected_deps: Vec<bool>,
    /// Selected file paths (set semantics, ordered for stable display).
    pub selected_file_paths: std::collections::BTreeSet<String>,
    /// Current filter query typed into the Files field.
    pub file_filter: String,
    pub ctx: FormContext,
    pub submitted: bool,
    pub cancelled: bool,
    pub due_error: bool,
    pub due_preset_idx: usize,
    /// Whether the `fzf` binary is available (set by `run_form`).
    pub fzf_available: bool,
    /// Set by `handle_key` to ask `run_form` to launch fzf for file picking.
    pub fzf_requested: bool,
}

/// A row shown in the (filtered) Files list.
pub struct FileRow {
    pub path: String,
    pub selected: bool,
    pub suggested: bool,
    /// True for the synthetic "add this typed path" row.
    pub add_custom: bool,
}

impl<'a> FormState<'a> {
    pub fn new(ctx: FormContext) -> Self {
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
        // There's always at least an empty list; start at the top.
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
        }
    }

    /// Candidate paths offered to fzf: project files plus any already-selected
    /// custom paths, de-duplicated.
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

    /// Build the rows currently visible in the Files field, honoring the filter.
    /// Includes a synthetic "add custom path" row when the typed filter doesn't
    /// already match an available or selected file.
    pub fn file_rows(&self) -> Vec<FileRow> {
        let q = self.file_filter.trim().to_lowercase();
        let suggested: std::collections::HashSet<&String> =
            self.ctx.suggested_files.iter().collect();

        let mut rows: Vec<FileRow> = vec![];
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Available project files matching the filter.
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
        // Selected custom paths (typed by the user, not in available_files).
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
        // If the typed path matches nothing, offer to add it verbatim.
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

    pub fn toggle_dep(&mut self) {
        if let Some(i) = self.dep_state.selected()
            && i < self.selected_deps.len()
        {
            self.selected_deps[i] = !self.selected_deps[i];
        }
    }

    pub fn toggle_file(&mut self) {
        let rows = self.file_rows();
        let Some(i) = self.file_state.selected() else {
            return;
        };
        let Some(row) = rows.get(i) else {
            return;
        };
        if row.add_custom {
            // Commit the typed path and clear the filter so the list resets.
            self.selected_file_paths.insert(row.path.clone());
            self.file_filter.clear();
            self.file_state.select(Some(0));
        } else if self.selected_file_paths.contains(&row.path) {
            self.selected_file_paths.remove(&row.path);
        } else {
            self.selected_file_paths.insert(row.path.clone());
        }
    }

    /// Clamp the Files list selection to the number of visible rows.
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
        self.due_error = !crate::infrastructure::dates::is_valid_due(&text);
    }

    fn set_due_text(&mut self, value: &str) {
        let mut ta = TextArea::default();
        ta.insert_str(value);
        self.due_area = ta;
        self.validate_due();
    }

    fn cycle_due(&mut self, forward: bool) {
        let presets = crate::infrastructure::dates::DUE_PRESETS;
        // Find the current preset index if the text matches one, else start fresh
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

    /// If focus is on one of the four text fields, feed the key to it (and
    /// re-validate Due), returning `true`. Otherwise leave state untouched and
    /// return `false` so the caller can handle non-text focuses.
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

    /// Apply a single key event to the form. Returns nothing; mutates state.
    /// Extracted from the event loop so it can be exercised in unit tests
    /// without a live terminal.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.cancelled = true;
            }
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                if self.can_submit() {
                    self.submitted = true;
                }
            }
            (KeyCode::Tab, _) => self.next_focus(),
            (KeyCode::BackTab, _) => self.prev_focus(),
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
                // In text fields, a space is just a character; other focuses
                // (Priority/Submit/Cancel) ignore it.
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
                // Move up within the list; if already at the top (or the list is
                // empty), fall through to the previous field so arrows can leave.
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
                // Move down within the list; if already at the bottom (or the
                // list is empty), fall through to the next field.
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
                // In the Files field, plain characters build a filter query.
                Focus::Files => {
                    if let KeyCode::Char(c) = key.code {
                        self.file_filter.push(c);
                        self.file_state.select(Some(0));
                        self.clamp_file_selection();
                    }
                }
                // Text fields route the key (incl. Due validation); other
                // focuses (Priority/Dependencies/Submit/Cancel) ignore it.
                _ => {
                    self.input_to_focused_text_field(key);
                }
            },
        }
    }

    pub fn collect_result(&self) -> FormInput {
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

// ── Entry point ───────────────────────────────────────────────────────────────

/// Run the form. Returns Some(FormInput) on submit, None on cancel.
pub fn run_form<B: Backend>(
    terminal: &mut Terminal<B>,
    ctx: FormContext,
) -> Result<Option<FormInput>> {
    let mut state = FormState::new(ctx);
    state.fzf_available = fzf::fzf_available();

    loop {
        terminal.draw(|f| render(f, &mut state))?;

        // Poll instead of a bare blocking read so a stray/odd event can never
        // leave the UI looking wedged: we always loop back and redraw.
        if !event::poll(std::time::Duration::from_millis(100))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            // Many terminals emit both Press and Release events; only act on Press
            // (and Repeat) to avoid every interaction firing twice.
            if key.kind == KeyEventKind::Release {
                continue;
            }
            state.handle_key(key);
        }

        if state.fzf_requested {
            state.fzf_requested = false;
            let candidates = state.fzf_candidates();
            // Hand the terminal back to fzf, then reclaim and force a redraw.
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

// ── Rendering ─────────────────────────────────────────────────────────────────

pub fn render(f: &mut Frame, state: &mut FormState) {
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
    // Heights: desc=3, proj=3, pri=3, due=3, tags=3, deps=5, files=7, buttons=3
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

    // Split: optional filter line + the list.
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
    let text = " Tab/Shift+Tab: move  •  ←/→: cycle priority  •  Space: toggle  •  Ctrl+S: save  •  Esc: cancel ";
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
