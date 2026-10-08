use crate::infrastructure::tui::theme::{Ink, ink};
use anyhow::Result;
use crossterm::event::KeyCode;
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use ratatui_textarea::TextArea;
use std::time::{Duration, Instant};

use crate::infrastructure::model::Priority;
use crate::infrastructure::tui::fzf;
use crate::infrastructure::tui::keymap::{self, Action};

#[derive(Debug, Clone, Default)]
pub struct FormInput {
    pub description: String,
    pub project: String,
    pub priority: Option<Priority>,
    pub due: String,
    pub tags: String,
    pub estimate: String,
    pub assignment: String,
    pub rationale: String,
    pub acceptance: String,
    pub verify: String,
    pub links: String,
    pub selected_deps: Vec<usize>,
    pub selected_files: Vec<String>,
}

impl FormInput {
    pub fn link_list(&self) -> Vec<String> {
        split_links(&self.links)
    }

    pub fn has_guide(&self) -> bool {
        [&self.assignment, &self.rationale, &self.acceptance]
            .iter()
            .any(|s| !s.trim().is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintKind {
    Task,
    Memory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub kind: HintKind,
    pub label: String,
    pub title: String,
}

pub type HintLookup<'a> = Box<dyn FnMut(&str) -> Vec<Hint> + 'a>;

pub struct FormContext<'a> {
    pub initial: FormInput,
    pub available_deps: Vec<(String, String)>,
    pub available_files: Vec<String>,
    pub suggested_dep_indices: Vec<usize>,
    pub suggested_files: Vec<String>,
    pub create: bool,
    pub lookup: Option<HintLookup<'a>>,
}

const HINT_DEBOUNCE: Duration = Duration::from_millis(350);
const HINT_MIN_CHARS: usize = 3;
const LABEL_W: u16 = 14;
const HINTS_MIN_WIDTH: u16 = 100;
const HINTS_W: u16 = 38;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Focus {
    Description,
    Project,
    Priority,
    Due,
    Tags,
    Estimate,
    Assignment,
    Rationale,
    Acceptance,
    Verify,
    Links,
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
    Focus::Estimate,
    Focus::Assignment,
    Focus::Rationale,
    Focus::Acceptance,
    Focus::Verify,
    Focus::Links,
    Focus::Dependencies,
    Focus::Files,
    Focus::Submit,
    Focus::Cancel,
];

impl Focus {
    fn is_guide(self) -> bool {
        matches!(
            self,
            Focus::Assignment | Focus::Rationale | Focus::Acceptance | Focus::Verify
        )
    }

    fn is_text(self) -> bool {
        !matches!(
            self,
            Focus::Priority | Focus::Dependencies | Focus::Files | Focus::Submit | Focus::Cancel
        )
    }

    fn label(self) -> &'static str {
        match self {
            Focus::Description => "Title",
            Focus::Project => "Project",
            Focus::Priority => "Priority",
            Focus::Due => "Due",
            Focus::Tags => "Tags",
            Focus::Estimate => "Estimate",
            Focus::Assignment => "Assignment",
            Focus::Rationale => "Why",
            Focus::Acceptance => "Done when",
            Focus::Verify => "Verify",
            Focus::Links => "Links",
            Focus::Dependencies => "Depends on",
            Focus::Files => "Files",
            Focus::Submit => "Save",
            Focus::Cancel => "Cancel",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Focus::Description => "what to do",
            Focus::Priority => "←/→ cycle",
            Focus::Due => "←/→ presets · friday, +3d",
            Focus::Tags => "comma-separated",
            Focus::Estimate => "90m, 1h30m",
            Focus::Assignment => "what was asked, verbatim",
            Focus::Rationale => "why it matters",
            Focus::Acceptance => "acceptance criterion",
            Focus::Verify => "command that proves it",
            Focus::Links => "space-separated URLs",
            _ => "",
        }
    }
}

fn split_links(s: &str) -> Vec<String> {
    s.split(|c: char| c.is_whitespace() || c == ',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

fn single_line(text: &str) -> TextArea<'static> {
    let mut ta = TextArea::default();
    ta.insert_str(text);
    ta.set_cursor_line_style(Style::default());
    ta
}

struct FormState<'a> {
    focus: Focus,
    desc_area: TextArea<'a>,
    project_area: TextArea<'a>,
    due_area: TextArea<'a>,
    tags_area: TextArea<'a>,
    estimate_area: TextArea<'a>,
    assignment_area: TextArea<'a>,
    rationale_area: TextArea<'a>,
    acceptance_area: TextArea<'a>,
    verify_area: TextArea<'a>,
    links_area: TextArea<'a>,
    priority: Option<Priority>,
    dep_state: ListState,
    file_state: ListState,
    selected_deps: Vec<bool>,
    selected_file_paths: std::collections::BTreeSet<String>,
    file_filter: String,
    ctx: FormContext<'a>,
    submitted: bool,
    cancelled: bool,
    due_error: bool,
    due_preset_idx: usize,
    fzf_available: bool,
    fzf_requested: bool,
    showing_help: bool,
    notice: Option<String>,
    hints: Vec<Hint>,
    hint_query: Option<String>,
    last_edit: Option<Instant>,
}

struct FileRow {
    path: String,
    selected: bool,
    suggested: bool,
    add_custom: bool,
}

impl<'a> FormState<'a> {
    fn new(ctx: FormContext<'a>) -> Self {
        let init = &ctx.initial;
        let n_deps = ctx.available_deps.len();

        let mut selected_deps = vec![false; n_deps];
        for &i in &init.selected_deps {
            if i < n_deps {
                selected_deps[i] = true;
            }
        }
        let selected_file_paths: std::collections::BTreeSet<String> =
            init.selected_files.iter().cloned().collect();

        let mut dep_state = ListState::default();
        if n_deps > 0 {
            dep_state.select(Some(0));
        }
        let mut file_state = ListState::default();
        if !ctx.available_files.is_empty() || !selected_file_paths.is_empty() {
            file_state.select(Some(0));
        }

        let mut state = FormState {
            focus: Focus::Description,
            desc_area: single_line(&init.description),
            project_area: single_line(&init.project),
            due_area: single_line(&init.due),
            tags_area: single_line(&init.tags),
            estimate_area: single_line(&init.estimate),
            assignment_area: single_line(&init.assignment),
            rationale_area: single_line(&init.rationale),
            acceptance_area: single_line(&init.acceptance),
            verify_area: single_line(&init.verify),
            links_area: single_line(&init.links),
            priority: init.priority.clone(),
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
            notice: None,
            hints: vec![],
            hint_query: None,
            last_edit: None,
        };
        state.validate_due();
        state
    }

    fn fields(&self) -> Vec<Focus> {
        ALL_FIELDS
            .iter()
            .copied()
            .filter(|f| self.ctx.create || !f.is_guide())
            .collect()
    }

    fn area(&self, f: Focus) -> Option<&TextArea<'a>> {
        Some(match f {
            Focus::Description => &self.desc_area,
            Focus::Project => &self.project_area,
            Focus::Due => &self.due_area,
            Focus::Tags => &self.tags_area,
            Focus::Estimate => &self.estimate_area,
            Focus::Assignment => &self.assignment_area,
            Focus::Rationale => &self.rationale_area,
            Focus::Acceptance => &self.acceptance_area,
            Focus::Verify => &self.verify_area,
            Focus::Links => &self.links_area,
            _ => return None,
        })
    }

    fn area_mut(&mut self, f: Focus) -> Option<&mut TextArea<'a>> {
        Some(match f {
            Focus::Description => &mut self.desc_area,
            Focus::Project => &mut self.project_area,
            Focus::Due => &mut self.due_area,
            Focus::Tags => &mut self.tags_area,
            Focus::Estimate => &mut self.estimate_area,
            Focus::Assignment => &mut self.assignment_area,
            Focus::Rationale => &mut self.rationale_area,
            Focus::Acceptance => &mut self.acceptance_area,
            Focus::Verify => &mut self.verify_area,
            Focus::Links => &mut self.links_area,
            _ => return None,
        })
    }

    fn text(&self, f: Focus) -> String {
        self.area(f).map(|a| a.lines().join("")).unwrap_or_default()
    }

    fn error(&self, f: Focus) -> Option<&'static str> {
        match f {
            Focus::Description if self.text(f).trim().is_empty() => Some("required"),
            Focus::Due if self.due_error => Some("invalid date"),
            Focus::Estimate => {
                let t = self.text(f);
                (!t.trim().is_empty()
                    && crate::infrastructure::util::dates::parse_duration_mins(&t).is_none())
                .then_some("use 90m or 1h30m")
            }
            Focus::Verify
                if self.ctx.create
                    && !self.text(Focus::Verify).trim().is_empty()
                    && self.text(Focus::Acceptance).trim().is_empty() =>
            {
                Some("needs a Done when")
            }
            Focus::Links => split_links(&self.text(f))
                .iter()
                .any(|l| !l.contains("://"))
                .then_some("expects URLs"),
            _ => None,
        }
    }

    fn first_error(&self) -> Option<(Focus, &'static str)> {
        self.fields()
            .into_iter()
            .find_map(|f| self.error(f).map(|e| (f, e)))
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
        let fields = self.fields();
        let idx = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = fields[(idx + 1) % fields.len()];
    }

    fn prev_focus(&mut self) {
        let fields = self.fields();
        let idx = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = fields[(idx + fields.len() - 1) % fields.len()];
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
        self.due_area = single_line(value);
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
        self.first_error().is_none()
    }

    fn try_submit(&mut self) {
        match self.first_error() {
            None => self.submitted = true,
            Some((f, e)) => {
                self.focus = f;
                self.notice = Some(format!("Cannot save · {}: {e}", f.label()));
            }
        }
    }

    fn input_to_focused_text_field(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let focus = self.focus;
        let Some(area) = self.area_mut(focus) else {
            return false;
        };
        let changed = area.input(key);
        if focus == Focus::Due {
            self.validate_due();
        }
        if focus == Focus::Description && changed {
            self.last_edit = Some(Instant::now());
        }
        true
    }

    fn refresh_hints(&mut self, now: Instant, force: bool) {
        let query = self.text(Focus::Description).trim().to_string();
        if self.hint_query.as_deref() == Some(query.as_str()) {
            return;
        }
        if !force
            && let Some(t) = self.last_edit
            && now.duration_since(t) < HINT_DEBOUNCE
        {
            return;
        }
        let Some(lookup) = self.ctx.lookup.as_mut() else {
            return;
        };
        self.hints = if query.chars().count() < HINT_MIN_CHARS {
            vec![]
        } else {
            lookup(&query)
        };
        self.hint_query = Some(query);
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if self.showing_help {
            self.showing_help = false;
            return;
        }
        self.notice = None;
        if key.code == KeyCode::Char('?') && !self.focus.is_text() {
            self.showing_help = true;
            return;
        }
        match keymap::control_action(key) {
            Some(Action::Cancel) => {
                self.cancelled = true;
                return;
            }
            Some(Action::Save) => {
                self.try_submit();
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
                Focus::Submit => self.try_submit(),
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
        let guide = |f: Focus| {
            if self.ctx.create {
                self.text(f)
            } else {
                String::new()
            }
        };
        FormInput {
            description: self.text(Focus::Description),
            project: self.text(Focus::Project),
            priority: self.priority.clone(),
            due: self.text(Focus::Due),
            tags: self.text(Focus::Tags),
            estimate: self.text(Focus::Estimate),
            assignment: guide(Focus::Assignment),
            rationale: guide(Focus::Rationale),
            acceptance: guide(Focus::Acceptance),
            verify: guide(Focus::Verify),
            links: self.text(Focus::Links),
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
    state.refresh_hints(Instant::now(), true);

    loop {
        terminal.draw(|f| render(f, &mut state))?;

        let key = crate::infrastructure::tui::next_key(100)?;
        if let Some(key) = key {
            state.handle_key(key);
        }
        state.refresh_hints(Instant::now(), false);
        if key.is_none() {
            continue;
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

pub(super) const MIN_SIZE: (u16, u16) = (40, 10);

fn render(f: &mut Frame, state: &mut FormState) {
    if crate::infrastructure::tui::screen::too_small(f, MIN_SIZE.0, MIN_SIZE.1) {
        return;
    }
    let area = f.area();
    let title = if state.ctx.create {
        " sara · new task "
    } else {
        " sara · edit task "
    };
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ink(Ink::Muted)))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(ink(Ink::Accent))
                    .add_modifier(Modifier::BOLD),
            )),
        area,
    );

    let inner = shrink(area, 1);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let show_hints = state.ctx.lookup.is_some() && inner.width >= HINTS_MIN_WIDTH;
    let (fields_area, hints_area) = if show_hints {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(HINTS_W)])
            .split(chunks[0]);
        (cols[0], Some(cols[1]))
    } else {
        (chunks[0], None)
    };

    render_fields(f, state, fields_area);
    if let Some(h) = hints_area {
        render_hints(f, state, h);
    }
    f.render_widget(Paragraph::new(footer_line(state)), chunks[1]);

    if state.showing_help {
        crate::infrastructure::tui::render_help_overlay(f, "Task form", &help_bindings());
    }
}

fn help_bindings() -> Vec<(&'static str, &'static str)> {
    let mut v = keymap::help::CONTROL_ACTION_BINDINGS.to_vec();
    v.extend([
        ("↑ / ↓", "move within a list, or to the previous/next field"),
        (
            "Enter",
            "next field · toggle · open fzf · save/cancel on buttons",
        ),
        ("Space", "toggle a dependency or file"),
        ("← / →", "cycle priority or a due-date preset"),
        ("?", "toggle this help (outside text fields)"),
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

enum Row {
    Section(&'static str),
    Field(Focus),
    Deps,
    Files,
    Gap,
    Buttons,
}

fn layout_rows(state: &FormState, height: u16) -> Vec<(Row, u16)> {
    let mut rows = vec![(Row::Section("1 · Task"), 1)];
    for f in [
        Focus::Description,
        Focus::Project,
        Focus::Priority,
        Focus::Due,
        Focus::Tags,
        Focus::Estimate,
    ] {
        rows.push((Row::Field(f), 1));
    }
    rows.push((Row::Gap, 1));
    let relations = if state.ctx.create {
        rows.push((Row::Section("2 · Guide"), 1));
        for f in [
            Focus::Assignment,
            Focus::Rationale,
            Focus::Acceptance,
            Focus::Verify,
        ] {
            rows.push((Row::Field(f), 1));
        }
        rows.push((Row::Gap, 1));
        "3 · Relations"
    } else {
        "2 · Relations"
    };
    rows.push((Row::Section(relations), 1));
    rows.push((Row::Field(Focus::Links), 1));
    let fixed = rows.len() as u16 + 4;
    let room = height.saturating_sub(fixed);
    let n_deps = state.ctx.available_deps.len().max(1) as u16;
    let n_files = state.file_rows().len().max(1) as u16;
    let deps_h = n_deps.min((room / 3).max(1)) + 1;
    let files_h = n_files.min(room.saturating_sub(deps_h).max(2)) + 1;
    rows.push((Row::Deps, deps_h));
    rows.push((Row::Files, files_h));
    rows.push((Row::Gap, 1));
    rows.push((Row::Buttons, 1));
    rows
}

fn row_has_focus(row: &Row, focus: Focus) -> bool {
    match row {
        Row::Field(f) => *f == focus,
        Row::Deps => focus == Focus::Dependencies,
        Row::Files => focus == Focus::Files,
        Row::Buttons => matches!(focus, Focus::Submit | Focus::Cancel),
        _ => false,
    }
}

fn render_fields(f: &mut Frame, state: &mut FormState, area: Rect) {
    let rows = layout_rows(state, area.height);
    let mut y = 0u16;
    let mut focus_end = 0u16;
    for (row, h) in &rows {
        y += h;
        if row_has_focus(row, state.focus) {
            focus_end = y;
        }
    }
    let offset = focus_end.saturating_sub(area.height);
    let mut y = 0u16;
    for (row, h) in &rows {
        let top = y;
        y += h;
        if top < offset || y - offset > area.height {
            continue;
        }
        let rect = Rect {
            x: area.x,
            y: area.y + top - offset,
            width: area.width,
            height: *h,
        };
        match row {
            Row::Section(t) => render_section(f, t, rect),
            Row::Field(Focus::Priority) => render_priority(f, state, rect),
            Row::Field(fo) => render_text_field(f, state, *fo, rect),
            Row::Deps => render_dependencies(f, state, rect),
            Row::Files => render_files(f, state, rect),
            Row::Gap => {}
            Row::Buttons => render_buttons(f, state, rect),
        }
    }
}

fn render_section(f: &mut Frame, title: &str, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ▌ ", Style::default().fg(ink(Ink::Accent))),
            Span::styled(
                title.to_string(),
                Style::default()
                    .fg(ink(Ink::Text))
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        area,
    );
}

fn label_spans(focus: Focus, focused: bool, error: bool) -> Vec<Span<'static>> {
    let marker = if focused { " ▸ " } else { "   " };
    let label_style = if error {
        Style::default().fg(ink(Ink::Err))
    } else if focused {
        Style::default()
            .fg(ink(Ink::Accent))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ink(Ink::Muted))
    };
    let width = LABEL_W as usize - 3;
    vec![
        Span::styled(
            marker,
            Style::default()
                .fg(ink(Ink::Accent))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{:<width$}", focus.label()), label_style),
    ]
}

fn split_field(area: Rect, side: &str) -> (Rect, Rect, Rect) {
    let side_w = if side.is_empty() {
        0
    } else {
        (side.chars().count() as u16 + 2).min(area.width / 3)
    };
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(LABEL_W),
            Constraint::Min(1),
            Constraint::Length(side_w),
        ])
        .split(area);
    (cols[0], cols[1], cols[2])
}

fn side_note(state: &FormState, focus: Focus) -> (String, Style) {
    if let Some(e) = state.error(focus) {
        return (format!("⚠\u{a0}{e} "), Style::default().fg(ink(Ink::Err)));
    }
    if state.focus == focus && !focus.hint().is_empty() {
        return (
            format!("{} ", focus.hint()),
            Style::default().fg(ink(Ink::Muted)),
        );
    }
    (String::new(), Style::default())
}

fn render_text_field(f: &mut Frame, state: &mut FormState, focus: Focus, area: Rect) {
    let focused = state.focus == focus;
    let (note, note_style) = side_note(state, focus);
    let (label_r, value_r, side_r) = split_field(area, &note);
    f.render_widget(
        Paragraph::new(Line::from(label_spans(
            focus,
            focused,
            state.error(focus).is_some(),
        ))),
        label_r,
    );
    if focused {
        if let Some(ta) = state.area_mut(focus) {
            ta.set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
            f.render_widget(&*ta, value_r);
        }
    } else {
        let text = state.text(focus);
        let span = if text.is_empty() {
            Span::styled("·", Style::default().fg(ink(Ink::Muted)))
        } else {
            Span::styled(text, Style::default().fg(ink(Ink::Text)))
        };
        f.render_widget(Paragraph::new(Line::from(span)), value_r);
    }
    if !note.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(note, note_style)))
                .alignment(ratatui::layout::Alignment::Right),
            side_r,
        );
    }
}

fn render_priority(f: &mut Frame, state: &mut FormState, area: Rect) {
    let focused = state.focus == Focus::Priority;
    let (note, note_style) = side_note(state, Focus::Priority);
    let (label_r, value_r, side_r) = split_field(area, &note);
    f.render_widget(
        Paragraph::new(Line::from(label_spans(Focus::Priority, focused, false))),
        label_r,
    );
    let (text, role) = match &state.priority {
        None => ("none", Ink::Muted),
        Some(Priority::L) => ("L\u{a0}low", Ink::Ok),
        Some(Priority::M) => ("M\u{a0}medium", Ink::Warn),
        Some(Priority::H) => ("H\u{a0}high", Ink::Err),
    };
    let arrow = Style::default().fg(if focused {
        ink(Ink::Accent)
    } else {
        ink(Ink::Muted)
    });
    let mut spans = vec![];
    if focused {
        spans.push(Span::styled("‹ ", arrow));
    }
    spans.push(Span::styled(
        text,
        Style::default().fg(ink(role)).add_modifier(Modifier::BOLD),
    ));
    if focused {
        spans.push(Span::styled(" ›", arrow));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), value_r);
    if !note.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(note, note_style)))
                .alignment(ratatui::layout::Alignment::Right),
            side_r,
        );
    }
}

fn list_header(f: &mut Frame, focus: Focus, focused: bool, extra: Vec<Span<'static>>, area: Rect) {
    let mut spans = label_spans(focus, focused, false);
    spans.extend(extra);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn split_list(area: Rect) -> (Rect, Rect) {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);
    let list = Rect {
        x: parts[1].x + 3,
        width: parts[1].width.saturating_sub(3),
        ..parts[1]
    };
    (parts[0], list)
}

fn checkbox_item(text: String, selected: bool, suggested: bool) -> ListItem<'static> {
    let (mark, mark_style) = if selected {
        ("[x] ", Style::default().fg(ink(Ink::Ok)))
    } else {
        ("[ ] ", Style::default().fg(ink(Ink::Muted)))
    };
    let text_style = if selected {
        Style::default().fg(ink(Ink::Text))
    } else if suggested {
        Style::default()
            .fg(ink(Ink::Muted))
            .add_modifier(Modifier::ITALIC)
    } else {
        Style::default().fg(ink(Ink::Muted))
    };
    ListItem::new(Line::from(vec![
        Span::styled(mark, mark_style),
        Span::styled(text, text_style),
    ]))
}

fn highlight(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(ink(Ink::Accent))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn count_badge(n: usize) -> Vec<Span<'static>> {
    if n == 0 {
        return vec![];
    }
    vec![Span::styled(
        format!("[{n} selected]"),
        Style::default().fg(ink(Ink::Ok)),
    )]
}

fn render_dependencies(f: &mut Frame, state: &mut FormState, area: Rect) {
    let focused = state.focus == Focus::Dependencies;
    let (head, list_r) = split_list(area);
    let n = state.selected_deps.iter().filter(|v| **v).count();
    list_header(f, Focus::Dependencies, focused, count_badge(n), head);
    if list_r.height == 0 {
        return;
    }
    if state.ctx.available_deps.is_empty() {
        f.render_widget(
            Paragraph::new("no open tasks").style(Style::default().fg(ink(Ink::Muted))),
            list_r,
        );
        return;
    }
    let items: Vec<ListItem> = state
        .ctx
        .available_deps
        .iter()
        .enumerate()
        .map(|(i, (id, desc))| {
            checkbox_item(
                format!("{id}\u{a0}{desc}"),
                state.selected_deps[i],
                state.ctx.suggested_dep_indices.contains(&i),
            )
        })
        .collect();
    let list = List::new(items)
        .highlight_style(highlight(focused))
        .highlight_symbol(if focused { "› " } else { "  " });
    f.render_stateful_widget(list, list_r, &mut state.dep_state);
}

fn render_files(f: &mut Frame, state: &mut FormState, area: Rect) {
    let focused = state.focus == Focus::Files;
    let (head, list_r) = split_list(area);
    let mut extra = count_badge(state.selected_file_paths.len());
    if focused && !state.file_filter.is_empty() {
        if !extra.is_empty() {
            extra.push(Span::raw("  "));
        }
        extra.push(Span::styled("/", Style::default().fg(ink(Ink::Accent))));
        extra.push(Span::styled(
            state.file_filter.clone(),
            Style::default().fg(ink(Ink::Text)),
        ));
    }
    list_header(f, Focus::Files, focused, extra, head);
    if list_r.height == 0 {
        return;
    }
    let file_rows = state.file_rows();
    if file_rows.is_empty() {
        let msg = if state.ctx.available_files.is_empty() {
            "no project files · type a path, Space to add"
        } else {
            "no matches"
        };
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(ink(Ink::Muted))),
            list_r,
        );
        return;
    }
    let items: Vec<ListItem> = file_rows
        .iter()
        .map(|r| {
            if r.add_custom {
                return ListItem::new(Span::styled(
                    format!("+ add \"{}\"", r.path),
                    Style::default().fg(ink(Ink::Special)),
                ));
            }
            checkbox_item(r.path.clone(), r.selected, r.suggested)
        })
        .collect();
    let list = List::new(items)
        .highlight_style(highlight(focused))
        .highlight_symbol(if focused { "› " } else { "  " });
    f.render_stateful_widget(list, list_r, &mut state.file_state);
}

fn button(label: &str, focused: bool, role: Ink, enabled: bool) -> Span<'static> {
    let style = if focused {
        Style::default()
            .bg(ink(role))
            .fg(ink(Ink::Base))
            .add_modifier(Modifier::BOLD)
    } else if enabled {
        Style::default().fg(ink(role)).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ink(Ink::Muted))
    };
    Span::styled(format!(" {label} "), style)
}

fn render_buttons(f: &mut Frame, state: &mut FormState, area: Rect) {
    let save_label = if state.ctx.create {
        "Create  ^S"
    } else {
        "Save  ^S"
    };
    let line = Line::from(vec![
        Span::raw("   "),
        button(
            save_label,
            state.focus == Focus::Submit,
            Ink::Ok,
            state.can_submit(),
        ),
        Span::raw("   "),
        button("Cancel  Esc", state.focus == Focus::Cancel, Ink::Err, true),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_hints(f: &mut Frame, state: &FormState, area: Rect) {
    let inner = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(1),
        ..area
    };
    let mut lines = vec![Line::from(vec![
        Span::styled("▌ ", Style::default().fg(ink(Ink::Accent))),
        Span::styled(
            "Similar work",
            Style::default()
                .fg(ink(Ink::Text))
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    lines.push(Line::raw(""));
    if state.hints.is_empty() {
        let msg = if state.hint_query.as_deref().unwrap_or("").chars().count() < HINT_MIN_CHARS {
            "type a title to see related tasks and memories"
        } else {
            "nothing similar yet"
        };
        lines.push(Line::from(Span::styled(
            msg,
            Style::default().fg(ink(Ink::Muted)),
        )));
    }
    let w = inner.width as usize;
    for h in &state.hints {
        let (kind, role) = match h.kind {
            HintKind::Task => ("task", Ink::Accent),
            HintKind::Memory => ("memory", Ink::Special),
        };
        let badge = format!("[{kind}\u{a0}{}]", h.label);
        let room = w.saturating_sub(badge.chars().count() + 1);
        lines.push(Line::from(vec![
            Span::styled(badge, Style::default().fg(ink(role))),
            Span::raw(" "),
            Span::styled(
                truncate_chars(&h.title, room),
                Style::default().fg(ink(Ink::Text)),
            ),
        ]));
    }
    f.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(ink(Ink::Muted))),
        area,
    );
    f.render_widget(
        Paragraph::new(lines),
        Rect {
            x: inner.x + 1,
            width: inner.width.saturating_sub(1),
            ..inner
        },
    );
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn footer_line(state: &FormState) -> Line<'static> {
    if let Some(n) = &state.notice {
        return Line::from(Span::styled(
            format!(" {n}"),
            Style::default()
                .fg(ink(Ink::Err))
                .add_modifier(Modifier::BOLD),
        ));
    }
    let keys: &[(&str, &str)] = match state.focus {
        Focus::Priority => &[
            ("←/→", "cycle"),
            ("Tab", "next"),
            ("^S", "save"),
            ("Esc", "cancel"),
            ("?", "help"),
        ],
        Focus::Due => &[
            ("←/→", "presets"),
            ("Tab", "next"),
            ("^S", "save"),
            ("Esc", "cancel"),
        ],
        Focus::Dependencies => &[
            ("↑/↓", "move"),
            ("Space", "toggle"),
            ("Tab", "next"),
            ("^S", "save"),
            ("?", "help"),
        ],
        Focus::Files if state.fzf_available => &[
            ("Enter", "fzf"),
            ("Space", "toggle"),
            ("type", "filter"),
            ("^S", "save"),
            ("?", "help"),
        ],
        Focus::Files => &[
            ("Space", "toggle"),
            ("type", "filter"),
            ("Tab", "next"),
            ("^S", "save"),
            ("?", "help"),
        ],
        Focus::Submit | Focus::Cancel => &[
            ("Enter", "confirm"),
            ("Tab", "next"),
            ("Esc", "cancel"),
            ("?", "help"),
        ],
        _ => &[
            ("Tab", "next"),
            ("S-Tab", "prev"),
            ("^S", "save"),
            ("Esc", "cancel"),
        ],
    };
    let key = Style::default()
        .fg(ink(Ink::Text))
        .add_modifier(Modifier::BOLD);
    let label = Style::default().fg(ink(Ink::Muted));
    let mut spans = vec![Span::raw(" ")];
    for (i, (k, d)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ·  ", label));
        }
        spans.push(Span::styled(k.to_string(), key));
        spans.push(Span::styled(format!(" {d}"), label));
    }
    Line::from(spans)
}

#[cfg(test)]
#[path = "../../../tests/unit/infrastructure/tui/review_form.rs"]
mod tests;
