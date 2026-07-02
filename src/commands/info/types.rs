use tui_textarea::TextArea;

use crate::infrastructure::model::Task;

pub struct Detail {
    pub task: Task,
    pub blocked_by: Vec<String>,
    pub blocking: Vec<String>,
    /// Display IDs of the tasks this task currently depends on (pending
    /// blockers). Used to pre-fill and reconcile the editable "Depends on" field.
    pub depends_on_ids: Vec<i64>,
    /// Files the user attached themselves.
    pub manual_files: Vec<String>,
    /// Files attached as suggestions.
    pub suggested_files: Vec<String>,
    pub links: Vec<crate::infrastructure::db::Link>,
    pub annotations: Vec<crate::infrastructure::db::Annotation>,
    pub history: Vec<crate::infrastructure::db::HistoryEntry>,
    /// Absolute project root, used to open relative file paths.
    pub project_root: Option<std::path::PathBuf>,
    /// Persisted branch snapshot (set via `sara addbranch`, populated on `sara stop`).
    pub branch: Option<crate::infrastructure::db::BranchRecord>,
    /// Tasks in the same project whose snapshot files overlap with this task's.
    pub overlaps: Vec<BranchOverlap>,
    /// Other pending tasks in the same project sharing at least one tag.
    pub similar: Vec<(i64, String, f64)>,
    /// Checklist items for this task.
    pub checklist: Vec<crate::infrastructure::db::ChecklistItem>,
    /// Urgency score components.
    pub urgency_breakdown: Option<crate::infrastructure::db::UrgencyBreakdown>,
    /// Daily activity counts for the task's project (last ~16 weeks).
    pub activity: std::collections::HashMap<chrono::NaiveDate, u32>,
    /// Aggregated stats for the project.
    pub stats: Option<crate::infrastructure::db::ProjectStats>,
    /// Guide fields: assignment, rationale, freshness, meta.
    pub guide: crate::infrastructure::db::TaskGuideFields,
    /// Code anchors (relevant files with reasons / symbols / lines).
    pub anchors: Vec<crate::infrastructure::db::Anchor>,
    /// AI run audit trail.
    pub ai_runs: Vec<crate::infrastructure::db::AiRun>,
    /// Current project HEAD commit, for the freshness banner.
    pub head_commit: Option<String>,
    /// Project-level setup/test/lint/run commands (verification context).
    pub project_commands: crate::infrastructure::db::ProjectCommands,
    /// The dependency chain (feature) this task belongs to, in blockers-first
    /// order. Empty when the task has no linked tasks. Used by the right-hand
    /// "Feature chain" panel to show progress and highlight the current task.
    pub chain: Vec<Task>,
}

pub struct BranchOverlap {
    pub id: i64,
    pub description: String,
    pub branch: String,
    pub shared_files: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum EditField {
    Description,
    Project,
    Priority,
    Due,
    Tags,
    Estimate,
    Recur,
    DependsOn,
}

pub(super) const EDIT_FIELDS: [EditField; 8] = [
    EditField::Description,
    EditField::Project,
    EditField::Priority,
    EditField::Due,
    EditField::Tags,
    EditField::Estimate,
    EditField::Recur,
    EditField::DependsOn,
];

impl EditField {
    pub(super) fn label(&self) -> &'static str {
        match self {
            EditField::Description => "Description",
            EditField::Project => "Project",
            EditField::Priority => "Priority",
            EditField::Due => "Due",
            EditField::Tags => "Tags",
            EditField::Estimate => "Estimate",
            EditField::Recur => "Recur",
            EditField::DependsOn => "Depends on",
        }
    }
}

/// Something the cursor can land on in the detail view.
#[derive(Clone, PartialEq)]
pub(super) enum Focusable {
    Field(EditField),
    File(String),
    Link(usize),
    Checklist(usize),
    /// Index into `d.anchors` (code anchors).
    Anchor(usize),
    /// Index into the task-level comment list (annotations where kind="comment").
    Comment(usize),
    /// Index into the flat list of typed notes (finding, constraint, …).
    Note(usize),
}

pub(super) struct EditState {
    pub(super) detail: Detail,
    pub(super) selected: usize,
    pub(super) editing: bool,
    /// True while typing a comment anchored to the focused element.
    pub(super) commenting: bool,
    /// True while typing the text of a new checklist step to add.
    pub(super) adding_step: bool,
    pub(super) editor: TextArea<'static>,
    pub(super) due_error: bool,
    /// Error from the last "Depends on" commit, shown until the next edit.
    pub(super) dep_error: Option<String>,
    pub(super) scroll: u16,
}

/// All typed notes in render order (finding, constraint, assumption, …).
pub(super) const NOTE_KINDS: [&str; 8] = [
    "finding",
    "constraint",
    "assumption",
    "open_question",
    "non_goal",
    "decision",
    "risk",
    "pattern",
];
