use tui_textarea::TextArea;

use crate::infrastructure::db::LinkFlags;
use crate::infrastructure::model::{Status, Task};

/// One node in the depth-1 dependency graph view: only what's needed for a
/// glanceable row (id, status glyph, PR/issue badge) — no description or
/// other metadata, per the "less is more" design goal for overview screens.
#[derive(Clone)]
pub(super) struct GraphNode {
    pub(super) uuid: uuid::Uuid,
    pub(super) id: Option<i64>,
    pub(super) status: Status,
    pub(super) badge: Option<LinkFlags>,
}

/// Depth-1 dependency graph data for the current task: every immediate
/// blocker and dependent, any status (so completed neighbors still show,
/// crossed out) — deliberately not the transitive closure. Kept uncapped
/// here; the render layer decides how many to actually show (capped by
/// default, all of them when the panel is "expanded") so toggling expand
/// doesn't need a re-fetch.
#[derive(Default, Clone)]
pub(super) struct DependencyGraph {
    /// "← blocked by".
    pub(super) blockers: Vec<GraphNode>,
    /// "blocks →".
    pub(super) dependents: Vec<GraphNode>,
}

impl DependencyGraph {
    pub(super) fn is_empty(&self) -> bool {
        self.blockers.is_empty() && self.dependents.is_empty()
    }
}

pub(super) struct Detail {
    pub(super) task: Task,
    pub(super) blocked_by: Vec<String>,
    pub(super) blocking: Vec<String>,
    /// Display IDs of the tasks this task currently depends on (pending
    /// blockers). Used to pre-fill and reconcile the editable "Depends on" field.
    pub(super) depends_on_ids: Vec<i64>,
    /// Files the user attached themselves.
    pub(super) manual_files: Vec<String>,
    /// Files attached as suggestions.
    pub(super) suggested_files: Vec<String>,
    pub(super) links: Vec<crate::infrastructure::db::Link>,
    pub(super) annotations: Vec<crate::infrastructure::db::Annotation>,
    pub(super) history: Vec<crate::infrastructure::db::HistoryEntry>,
    /// Absolute project root, used to open relative file paths.
    pub(super) project_root: Option<std::path::PathBuf>,
    /// Persisted branch snapshot (set via `sara addbranch`, populated on `sara stop`).
    pub(super) branch: Option<crate::infrastructure::db::BranchRecord>,
    /// Tasks in the same project whose snapshot files overlap with this task's.
    pub(super) overlaps: Vec<BranchOverlap>,
    /// Other pending tasks in the same project sharing at least one tag.
    pub(super) similar: Vec<(i64, String, f64)>,
    /// Checklist items for this task.
    pub(super) checklist: Vec<crate::infrastructure::db::ChecklistItem>,
    /// Urgency score components.
    pub(super) urgency_breakdown: Option<crate::infrastructure::db::UrgencyBreakdown>,
    /// Daily activity counts for the task's project (last ~16 weeks).
    pub(super) activity: std::collections::HashMap<chrono::NaiveDate, u32>,
    /// Aggregated stats for the project.
    pub(super) stats: Option<crate::infrastructure::db::ProjectStats>,
    /// Guide fields: assignment, rationale, freshness, meta.
    pub(super) guide: crate::infrastructure::db::TaskGuideFields,
    /// Code anchors (relevant files with reasons / symbols / lines).
    pub(super) anchors: Vec<crate::infrastructure::db::Anchor>,
    /// AI run audit trail.
    pub(super) ai_runs: Vec<crate::infrastructure::db::AiRun>,
    /// Current project HEAD commit, for the freshness banner.
    pub(super) head_commit: Option<String>,
    /// Project-level setup/test/lint/run commands (verification context).
    pub(super) project_commands: crate::infrastructure::db::ProjectCommands,
    /// The dependency chain (feature) this task belongs to, in blockers-first
    /// order. Empty when the task has no linked tasks. Used by the right-hand
    /// "Feature chain" panel to show progress and highlight the current task.
    pub(super) chain: Vec<Task>,
    /// Depth-1 blockers/dependents for the dependency graph panel (toggled
    /// with 'd', replacing the chain panel when active).
    pub(super) graph: DependencyGraph,
}

pub(super) struct BranchOverlap {
    pub(super) id: i64,
    pub(super) description: String,
    pub(super) branch: String,
    pub(super) shared_files: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum EditField {
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
    /// True while the dependency-graph panel is shown instead of the chain panel.
    pub(super) show_graph: bool,
    /// True to show every neighbor on the graph's overflowing side(s) instead
    /// of the capped "+N more" summary row.
    pub(super) graph_expanded: bool,
    /// True while showing the opt-in "full impact" (transitive blockers) view.
    pub(super) graph_full_impact: bool,
    /// Transitive blockers for the "full impact" view, computed on demand
    /// when `graph_full_impact` is toggled on (not kept in `Detail` — it's a
    /// UI-triggered expansion, not part of the always-current snapshot).
    pub(super) full_impact: Vec<GraphNode>,
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
