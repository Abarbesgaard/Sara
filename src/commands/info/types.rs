use tui_textarea::TextArea;

use crate::infrastructure::db::LinkFlags;
use crate::infrastructure::model::{Status, Task};

/// One node in the task tree: only what's needed for a glanceable row (id,
/// status glyph, PR/issue badge) — no description, per the "less is more"
/// design goal for overview screens — plus its own children one hop further
/// in the same direction, so the tree can actually branch instead of
/// flattening a DAG into a single line.
#[derive(Clone)]
pub(super) struct GraphNode {
    pub(super) uuid: uuid::Uuid,
    pub(super) id: Option<i64>,
    pub(super) status: Status,
    pub(super) badge: Option<LinkFlags>,
    /// Short label shown next to the id — unlike the old depth-1 graph, the
    /// tree is now the primary "how does this relate to other tasks" view,
    /// so it earns back the description the depth-1 version deliberately
    /// dropped.
    pub(super) description: String,
    /// Further blockers-of-blockers / dependents-of-dependents, bounded by
    /// the depth and node-count caps used when the tree was built.
    pub(super) children: Vec<GraphNode>,
    /// Direct children that exist but were cut off by the per-node fan-out
    /// cap (not by depth) — rendered as a trailing "+N more" leaf.
    pub(super) hidden_children: usize,
}

/// The full task-relationship tree for the current task: every hop of
/// blockers ("← blocked by", recursively) and every hop of dependents
/// ("blocks →", recursively). This is the primary answer to "how is this
/// task tied to others" — replacing the old flat "feature chain" list, which
/// flattened a branching DAG into a single line and lost the structure.
#[derive(Default, Clone)]
pub(super) struct TaskTree {
    pub(super) blockers: Vec<GraphNode>,
    /// Direct blockers beyond the per-node fan-out cap, not included above.
    pub(super) blockers_hidden: usize,
    pub(super) dependents: Vec<GraphNode>,
    /// Direct dependents beyond the per-node fan-out cap, not included above.
    pub(super) dependents_hidden: usize,
}

impl TaskTree {
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
    /// The task tree: full blockers/dependents structure for the right-hand
    /// "Task tree" panel, always shown (compact by default, 'd' expands it).
    pub(super) tree: TaskTree,
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
    /// `selected` as of the previous frame — lets render() detect navigation
    /// and pull the viewport along only then, so manual PageUp/PageDown
    /// scrolling isn't snapped back to the selection every frame.
    pub(super) last_selected: Option<usize>,
    /// True to render the task tree at full depth/fan-out instead of the
    /// compact default (2 levels, a handful of siblings per node).
    pub(super) tree_expanded: bool,
    /// True to show the urgency score's additive breakdown (pri/due/blocking/…)
    /// instead of just the bare number. Off by default — the formula is
    /// rarely needed at a glance, only when the score looks surprising.
    pub(super) show_urgency_breakdown: bool,
    /// True to show long text in full (assignment/rationale/typed notes) and
    /// every checklist item's intent/verify/result detail, instead of the
    /// collapsed default (truncated text, details only on the selected row).
    pub(super) verbose: bool,
    /// True to show the AI's execution workpaper (findings, constraints,
    /// assumptions, decisions, …) in full. Off by default — a human
    /// reviewing the task wants impact/risk/what's-next, not the AI's own
    /// scratch notes from working the problem; "risk" notes are the
    /// exception and always show regardless of this toggle.
    pub(super) show_notes: bool,
}

/// All typed notes in render order. "risk" is first and deliberately not a
/// process note: it's the one kind a human reviewer (not the AI executing
/// the task) actually wants to see by default — the rest are the AI's own
/// execution workpaper and stay collapsed behind `EditState::show_notes`.
pub(super) const NOTE_KINDS: [&str; 8] = [
    "risk",
    "finding",
    "constraint",
    "assumption",
    "open_question",
    "non_goal",
    "decision",
    "pattern",
];
