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

use crate::model::Priority;

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
    /// Which deps are "suggested" by LLM (shown dim until user acts)
    pub suggested_dep_indices: Vec<usize>,
    /// File paths "suggested" by the LLM (shown dim/italic).
    pub suggested_files: Vec<String>,
    /// If LLM enrichment failed, the error message to show in the form header.
    pub llm_status: Option<String>,
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
    /// Selected file paths (set semantics, ordered for stable display).
    selected_file_paths: std::collections::BTreeSet<String>,
    /// Current filter query typed into the Files field.
    file_filter: String,
    ctx: FormContext,
    submitted: bool,
    cancelled: bool,
    due_error: bool,
    due_preset_idx: usize,
    /// Whether the `fzf` binary is available (set by `run_form`).
    fzf_available: bool,
    /// Set by `handle_key` to ask `run_form` to launch fzf for file picking.
    fzf_requested: bool,
}

/// A row shown in the (filtered) Files list.
struct FileRow {
    path: String,
    selected: bool,
    suggested: bool,
    /// True for the synthetic "add this typed path" row.
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
    fn file_rows(&self) -> Vec<FileRow> {
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
        self.due_error = !crate::dates::is_valid_due(&text);
    }

    fn set_due_text(&mut self, value: &str) {
        let mut ta = TextArea::default();
        ta.insert_str(value);
        self.due_area = ta;
        self.validate_due();
    }

    fn cycle_due(&mut self, forward: bool) {
        let presets = crate::dates::DUE_PRESETS;
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

    /// Apply a single key event to the form. Returns nothing; mutates state.
    /// Extracted from the event loop so it can be exercised in unit tests
    /// without a live terminal.
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
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
                // In text fields, a space is just a character.
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
                _ => {}
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
                // In the Files field, plain characters build a filter query.
                Focus::Files => {
                    if let KeyCode::Char(c) = key.code {
                        self.file_filter.push(c);
                        self.file_state.select(Some(0));
                        self.clamp_file_selection();
                    }
                }
                _ => {}
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

// ── Entry point ───────────────────────────────────────────────────────────────

/// Run the form. Returns Some(FormInput) on submit, None on cancel.
pub fn run_form<B: Backend>(
    terminal: &mut Terminal<B>,
    ctx: FormContext,
) -> Result<Option<FormInput>> {
    let mut state = FormState::new(ctx);
    state.fzf_available = fzf_available();

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
            crate::tui::suspend()?;
            let picked = run_fzf(&candidates, &state.file_filter);
            crate::tui::resume()?;
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

/// Whether the `fzf` binary is on PATH.
fn fzf_available() -> bool {
    std::process::Command::new("fzf")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run fzf as a multi-select picker over `candidates`, pre-filling `query`.
/// Returns the chosen paths, or None if fzf was cancelled or failed.
fn run_fzf(candidates: &[String], query: &str) -> Option<Vec<String>> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("fzf")
        .args([
            "--multi", "--prompt", "files> ", "--height", "100%", "--border",
        ])
        .arg("--query")
        .arg(query)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;

    if let Some(stdin) = child.stdin.as_mut() {
        for c in candidates {
            let _ = writeln!(stdin, "{c}");
        }
    }

    let output = child.wait_with_output().ok()?;
    // fzf exits 130 when the user aborts (Esc/Ctrl-C); keep selection unchanged.
    if !output.status.success() {
        return None;
    }
    let selected: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Some(selected)
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render(f: &mut Frame, state: &mut FormState) {
    let area = f.area();
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" sara — Review Task "),
        area,
    );

    let inner = shrink(area, 1);

    // If there's an LLM status message, reserve one line for it.
    let (status_area, fields_area) = if state.ctx.llm_status.is_some() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);
        (Some(chunks[0]), chunks[1])
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);
        (None, chunks[0])
    };

    // Footer is always last — re-split to get it
    let footer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let footer = *footer_chunks.last().unwrap();

    if let (Some(sa), Some(msg)) = (status_area, &state.ctx.llm_status) {
        let short = if msg.len() > sa.width as usize {
            format!("⚠ LLM: {}…", &msg[..sa.width.saturating_sub(10) as usize])
        } else {
            format!("⚠ LLM: {msg}")
        };
        f.render_widget(
            Paragraph::new(short).style(Style::default().fg(Color::Yellow)),
            sa,
        );
    }

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

fn render_fields(f: &mut Frame, state: &mut FormState, area: Rect) {
    // Heights: desc=3, proj=3, pri=3, due=3, tags=3, deps=5, files=7, buttons=3
    let heights = [3u16, 3, 3, 3, 3, 5, 7, 3];
    let _total: u16 = heights.iter().sum();
    if area.height < 4 {
        return;
    }

    let constraints: Vec<Constraint> = heights.iter().map(|&h| Constraint::Length(h)).collect();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    // ── Description
    {
        let focused = state.focus == Focus::Description;
        let block = field_block("Description", focused);
        let inner = block.inner(rows[0]);
        f.render_widget(block, rows[0]);
        state.desc_area.set_block(Block::default());
        f.render_widget(&state.desc_area, inner);
    }

    // ── Project
    {
        let focused = state.focus == Focus::Project;
        let block = field_block("Project", focused);
        let inner = block.inner(rows[1]);
        f.render_widget(block, rows[1]);
        state.project_area.set_block(Block::default());
        f.render_widget(&state.project_area, inner);
    }

    // ── Priority
    {
        let focused = state.focus == Focus::Priority;
        let block = field_block("Priority  ←/→ to cycle", focused);
        let inner = block.inner(rows[2]);
        f.render_widget(block, rows[2]);
        let label = match &state.priority {
            None => Span::styled("None", Style::default().fg(Color::DarkGray)),
            Some(Priority::L) => Span::styled("L  (Low)", Style::default().fg(Color::Green)),
            Some(Priority::M) => Span::styled("M  (Medium)", Style::default().fg(Color::Yellow)),
            Some(Priority::H) => Span::styled("H  (High)", Style::default().fg(Color::Red)),
        };
        f.render_widget(Paragraph::new(Line::from(label)), inner);
    }

    // ── Due
    {
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
        let inner = block.inner(rows[3]);
        f.render_widget(block, rows[3]);
        state.due_area.set_block(Block::default());
        f.render_widget(&state.due_area, inner);
    }

    // ── Tags
    {
        let focused = state.focus == Focus::Tags;
        let block = field_block("Tags  (comma-separated)", focused);
        let inner = block.inner(rows[4]);
        f.render_widget(block, rows[4]);
        state.tags_area.set_block(Block::default());
        f.render_widget(&state.tags_area, inner);
    }

    // ── Dependencies
    {
        let focused = state.focus == Focus::Dependencies;
        let block = field_block("Dependencies  (↑/↓ move, space toggle)", focused);
        let inner = block.inner(rows[5]);
        f.render_widget(block, rows[5]);
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

    // ── Files
    {
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
        let inner = block.inner(rows[6]);
        f.render_widget(block, rows[6]);

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

    // ── Submit / Cancel
    {
        let row = rows[7];
        let halves = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(row);

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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn ctx_with_deps() -> FormContext {
        FormContext {
            initial: FormInput {
                description: "test".into(),
                project: "tk".into(),
                priority: Some(Priority::L),
                due: "today".into(),
                tags: "a,b".into(),
                selected_deps: vec![],
                selected_files: vec![],
            },
            available_deps: vec![("1".into(), "jshfgklfhg".into())],
            available_files: vec!["Cargo.toml".into(), "README.md".into()],
            suggested_dep_indices: vec![],
            suggested_files: vec![],
            llm_status: None,
        }
    }

    /// Serialize a rendered TestBackend buffer into text: one line per row,
    /// trailing whitespace trimmed, rows joined by '\n'. Captures the visible
    /// glyphs (layout + labels) without styling — enough to pin the layout.
    fn buffer_to_string(terminal: &Terminal<TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        let area = *buf.area();
        let mut out = String::new();
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }

    #[test]
    #[ignore = "capture helper: run with --ignored to (re)write snapshot files"]
    fn write_render_snapshots() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tui/snapshots");
        std::fs::create_dir_all(dir).unwrap();

        let mut state = FormState::new(ctx_with_deps());
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| render(f, &mut state)).unwrap();
        std::fs::write(
            format!("{dir}/review_form_normal.txt"),
            buffer_to_string(&terminal),
        )
        .unwrap();

        let mut ctx = ctx_with_deps();
        ctx.llm_status = Some("model unavailable, used heuristics".into());
        let mut state = FormState::new(ctx);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| render(f, &mut state)).unwrap();
        std::fs::write(
            format!("{dir}/review_form_llm_status.txt"),
            buffer_to_string(&terminal),
        )
        .unwrap();

        let mut state = FormState::new(ctx_with_deps());
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| render(f, &mut state)).unwrap();
        std::fs::write(
            format!("{dir}/review_form_small.txt"),
            buffer_to_string(&terminal),
        )
        .unwrap();
    }

    /// Characterization (snapshot) tests for `render`. These pin the visible
    /// layout and labels so the planned `render_fields` split stays behaviour-
    /// identical. To intentionally update them, run:
    ///   cargo test write_render_snapshots -- --ignored
    /// and review the diff under src/tui/snapshots/.
    #[test]
    fn render_normal_matches_snapshot() {
        let mut state = FormState::new(ctx_with_deps());
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| render(f, &mut state)).unwrap();
        assert_eq!(
            buffer_to_string(&terminal),
            // Normalize CRLF: git may check the snapshot out with \r\n on Windows.
            include_str!("snapshots/review_form_normal.txt").replace("\r\n", "\n"),
        );
    }

    #[test]
    fn render_with_llm_status_matches_snapshot() {
        let mut ctx = ctx_with_deps();
        ctx.llm_status = Some("model unavailable, used heuristics".into());
        let mut state = FormState::new(ctx);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| render(f, &mut state)).unwrap();
        assert_eq!(
            buffer_to_string(&terminal),
            include_str!("snapshots/review_form_llm_status.txt").replace("\r\n", "\n"),
        );
    }

    #[test]
    fn render_small_terminal_matches_snapshot() {
        let mut state = FormState::new(ctx_with_deps());
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| render(f, &mut state)).unwrap();
        assert_eq!(
            buffer_to_string(&terminal),
            include_str!("snapshots/review_form_small.txt").replace("\r\n", "\n"),
        );
    }

    #[test]
    fn render_with_toggled_dep_does_not_panic() {
        let mut state = FormState::new(ctx_with_deps());
        state.focus = Focus::Dependencies;
        state.toggle_dep();
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &mut state)).unwrap();
    }

    #[test]
    fn render_with_toggled_file_does_not_panic() {
        let mut state = FormState::new(ctx_with_deps());
        state.focus = Focus::Files;
        state.toggle_file();
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &mut state)).unwrap();
    }

    #[test]
    fn render_small_terminal_does_not_panic() {
        let mut state = FormState::new(ctx_with_deps());
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &mut state)).unwrap();
    }

    // ── Key handling ──────────────────────────────────────────────────────────

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn tab_to(state: &mut FormState, target: Focus) {
        // Tab forward at most one full cycle to reach `target`.
        for _ in 0..ALL_FIELDS.len() + 1 {
            if state.focus == target {
                return;
            }
            state.handle_key(key(KeyCode::Tab));
        }
        panic!("never reached focus {target:?}");
    }

    #[test]
    fn tab_cycles_through_all_fields_and_wraps() {
        let mut state = FormState::new(ctx_with_deps());
        assert_eq!(state.focus, Focus::Description);
        for expected in ALL_FIELDS.iter().skip(1) {
            state.handle_key(key(KeyCode::Tab));
            assert_eq!(state.focus, *expected);
        }
        // Wrap back to the start.
        state.handle_key(key(KeyCode::Tab));
        assert_eq!(state.focus, Focus::Description);
    }

    /// The reported softlock: toggle a dependency with Space, then confirm
    /// every subsequent key still moves focus (i.e. the form is not wedged).
    #[test]
    fn space_toggle_dep_then_tab_still_moves_focus() {
        let mut state = FormState::new(ctx_with_deps());
        tab_to(&mut state, Focus::Dependencies);

        state.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(
            state.selected_deps,
            vec![true],
            "space should toggle dep on"
        );
        assert_eq!(
            state.focus,
            Focus::Dependencies,
            "toggle must not move focus"
        );

        // Now navigation must still work.
        state.handle_key(key(KeyCode::Tab));
        assert_eq!(state.focus, Focus::Files);
        state.handle_key(key(KeyCode::Tab));
        assert_eq!(state.focus, Focus::Submit);
    }

    /// Same as above but with Enter (the user also tried Enter on deps).
    #[test]
    fn enter_toggle_dep_then_tab_still_moves_focus() {
        let mut state = FormState::new(ctx_with_deps());
        tab_to(&mut state, Focus::Dependencies);

        state.handle_key(key(KeyCode::Enter));
        assert_eq!(state.selected_deps, vec![true]);
        assert_eq!(state.focus, Focus::Dependencies);

        state.handle_key(key(KeyCode::Tab));
        assert_eq!(state.focus, Focus::Files);
    }

    #[test]
    fn space_toggles_dep_on_and_off() {
        let mut state = FormState::new(ctx_with_deps());
        tab_to(&mut state, Focus::Dependencies);
        state.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(state.selected_deps, vec![true]);
        state.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(state.selected_deps, vec![false]);
    }

    #[test]
    fn arrows_navigate_within_multi_item_file_list() {
        let mut state = FormState::new(ctx_with_deps());
        tab_to(&mut state, Focus::Files);
        assert_eq!(state.file_state.selected(), Some(0));
        // Down moves within the 2-item list, focus stays on Files.
        state.handle_key(key(KeyCode::Down));
        assert_eq!(state.file_state.selected(), Some(1));
        assert_eq!(state.focus, Focus::Files);
        // Up moves back within the list, focus stays.
        state.handle_key(key(KeyCode::Up));
        assert_eq!(state.file_state.selected(), Some(0));
        assert_eq!(state.focus, Focus::Files);
    }

    /// The exact reported bug: Down on a single-item Dependencies list must
    /// move focus to Files instead of getting stuck.
    #[test]
    fn down_from_single_item_dependencies_moves_to_files() {
        let mut state = FormState::new(ctx_with_deps()); // 1 dependency
        tab_to(&mut state, Focus::Dependencies);
        state.handle_key(key(KeyCode::Down));
        assert_eq!(state.focus, Focus::Files);
    }

    #[test]
    fn up_from_top_of_dependencies_moves_to_previous_field() {
        let mut state = FormState::new(ctx_with_deps());
        tab_to(&mut state, Focus::Dependencies);
        state.handle_key(key(KeyCode::Up));
        assert_eq!(state.focus, Focus::Tags);
    }

    /// At the bottom of a multi-item list, Down should leave the list.
    #[test]
    fn down_at_bottom_of_file_list_moves_to_next_field() {
        let mut state = FormState::new(ctx_with_deps()); // 2 files
        tab_to(&mut state, Focus::Files);
        state.handle_key(key(KeyCode::Down)); // -> index 1 (last)
        assert_eq!(state.focus, Focus::Files);
        state.handle_key(key(KeyCode::Down)); // at bottom -> leave
        assert_eq!(state.focus, Focus::Submit);
    }

    #[test]
    fn down_then_up_round_trips_dependencies_and_files() {
        let mut state = FormState::new(ctx_with_deps());
        tab_to(&mut state, Focus::Dependencies);
        state.handle_key(key(KeyCode::Down)); // deps -> files (single dep)
        assert_eq!(state.focus, Focus::Files);
        state.handle_key(key(KeyCode::Up)); // files top -> back to deps
        assert_eq!(state.focus, Focus::Dependencies);
    }

    #[test]
    fn toggle_second_file_via_navigation_then_space() {
        let mut state = FormState::new(ctx_with_deps());
        tab_to(&mut state, Focus::Files);
        state.handle_key(key(KeyCode::Down)); // highlight README.md (row 1)
        state.handle_key(key(KeyCode::Char(' ')));
        assert!(state.selected_file_paths.contains("README.md"));
        assert_eq!(
            state.collect_result().selected_files,
            vec!["README.md".to_string()]
        );
    }

    #[test]
    fn typing_filters_file_list() {
        let mut state = FormState::new(ctx_with_deps());
        tab_to(&mut state, Focus::Files);
        // Type "read" -> only README.md matches.
        for c in "read".chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }
        let rows = state.file_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "README.md");
        assert!(!rows[0].add_custom);
        // Toggle the single match on.
        state.handle_key(key(KeyCode::Char(' ')));
        assert!(state.selected_file_paths.contains("README.md"));
    }

    #[test]
    fn backspace_edits_filter() {
        let mut state = FormState::new(ctx_with_deps());
        tab_to(&mut state, Focus::Files);
        for c in "xyz".chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }
        state.handle_key(key(KeyCode::Backspace));
        assert_eq!(state.file_filter, "xy");
    }

    #[test]
    fn typing_unknown_path_offers_add_custom_row() {
        let mut state = FormState::new(ctx_with_deps());
        tab_to(&mut state, Focus::Files);
        for c in "src/new.rs".chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }
        let rows = state.file_rows();
        // No project file matches, so only the synthetic add row is present.
        assert_eq!(rows.len(), 1);
        assert!(rows[0].add_custom);
        assert_eq!(rows[0].path, "src/new.rs");
        // Enter adds it (no fzf in tests) and clears the filter.
        state.handle_key(key(KeyCode::Enter));
        assert!(state.selected_file_paths.contains("src/new.rs"));
        assert_eq!(state.file_filter, "");
        assert_eq!(
            state.collect_result().selected_files,
            vec!["src/new.rs".to_string()]
        );
    }

    #[test]
    fn enter_requests_fzf_when_available() {
        let mut state = FormState::new(ctx_with_deps());
        state.fzf_available = true;
        tab_to(&mut state, Focus::Files);
        state.handle_key(key(KeyCode::Enter));
        assert!(state.fzf_requested);
        // Nothing toggled directly; fzf handles selection in run_form.
        assert!(state.selected_file_paths.is_empty());
    }

    #[test]
    fn preselected_files_round_trip() {
        let mut ctx = ctx_with_deps();
        ctx.initial.selected_files = vec!["Cargo.toml".into()];
        ctx.suggested_files = vec!["Cargo.toml".into()];
        let state = FormState::new(ctx);
        assert_eq!(
            state.collect_result().selected_files,
            vec!["Cargo.toml".to_string()]
        );
    }

    /// Full flow: toggle a dep, then reach Submit and submit with Ctrl+S.
    #[test]
    fn can_submit_after_toggling_dep() {
        let mut state = FormState::new(ctx_with_deps());
        tab_to(&mut state, Focus::Dependencies);
        state.handle_key(key(KeyCode::Char(' ')));
        state.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(state.submitted);
        assert_eq!(state.collect_result().selected_deps, vec![0]);
    }

    #[test]
    fn esc_cancels_even_from_dependencies() {
        let mut state = FormState::new(ctx_with_deps());
        tab_to(&mut state, Focus::Dependencies);
        state.handle_key(key(KeyCode::Char(' ')));
        state.handle_key(key(KeyCode::Esc));
        assert!(state.cancelled);
    }

    /// Regression: a space inside a text field must insert a space, not be
    /// swallowed by the dependency/file toggle arm.
    #[test]
    fn space_in_text_field_inserts_space() {
        let mut state = FormState::new(ctx_with_deps());
        assert_eq!(state.focus, Focus::Description);
        // desc_area is pre-filled with "test"; append " a b".
        state.handle_key(key(KeyCode::Char(' ')));
        state.handle_key(key(KeyCode::Char('a')));
        state.handle_key(key(KeyCode::Char(' ')));
        state.handle_key(key(KeyCode::Char('b')));
        assert_eq!(state.desc_area.lines().join(""), "test a b");
    }

    #[test]
    fn empty_deps_list_toggle_is_noop() {
        let mut ctx = ctx_with_deps();
        ctx.available_deps = vec![];
        let mut state = FormState::new(ctx);
        tab_to(&mut state, Focus::Dependencies);
        state.handle_key(key(KeyCode::Char(' ')));
        // Down on an empty list falls straight through to the next field.
        state.handle_key(key(KeyCode::Down));
        assert_eq!(state.focus, Focus::Files);
    }
}
