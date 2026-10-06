use ratatui_textarea::TextArea;

use crate::infrastructure::db::LinkFlags;
use crate::infrastructure::model::{Status, Task};

#[derive(Clone)]
pub(super) struct GraphNode {
    pub(super) uuid: uuid::Uuid,
    pub(super) id: Option<i64>,
    pub(super) status: Status,
    pub(super) badge: Option<LinkFlags>,
    pub(super) description: String,
    pub(super) children: Vec<GraphNode>,
    pub(super) hidden_children: usize,
}

#[derive(Default, Clone)]
pub(super) struct TaskTree {
    pub(super) blockers: Vec<GraphNode>,
    pub(super) blockers_hidden: usize,
    pub(super) dependents: Vec<GraphNode>,
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
    pub(super) cited: Vec<String>,
    pub(super) depends_on_ids: Vec<i64>,
    pub(super) manual_files: Vec<String>,
    pub(super) suggested_files: Vec<String>,
    pub(super) links: Vec<crate::infrastructure::db::Link>,
    pub(super) annotations: Vec<crate::infrastructure::db::Annotation>,
    pub(super) history: Vec<crate::infrastructure::db::HistoryEntry>,
    pub(super) project_root: Option<std::path::PathBuf>,
    pub(super) branch: Option<crate::infrastructure::db::BranchRecord>,
    pub(super) similar: Vec<(i64, String, f64)>,
    pub(super) checklist: Vec<crate::infrastructure::db::ChecklistItem>,
    pub(super) urgency_breakdown: Option<crate::infrastructure::db::UrgencyBreakdown>,
    pub(super) activity: std::collections::HashMap<chrono::NaiveDate, u32>,
    pub(super) stats: Option<crate::infrastructure::db::ProjectStats>,
    pub(super) guide: crate::infrastructure::db::TaskGuideFields,
    pub(super) anchors: Vec<crate::infrastructure::db::Anchor>,
    pub(super) ai_runs: Vec<crate::infrastructure::db::AiRun>,
    pub(super) head_commit: Option<String>,
    pub(super) project_commands: crate::infrastructure::db::ProjectCommands,
    pub(super) tree: TaskTree,
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

#[derive(Clone, PartialEq)]
pub(super) enum Focusable {
    Field(EditField),
    File(String),
    Link(usize),
    Checklist(usize),
    Anchor(usize),
    Comment(usize),
    Note(usize),
}

pub(super) struct EditState {
    pub(super) detail: Detail,
    pub(super) selected: usize,
    pub(super) editing: bool,
    pub(super) commenting: bool,
    pub(super) adding_step: bool,
    pub(super) editor: TextArea<'static>,
    pub(super) due_error: bool,
    pub(super) dep_error: Option<String>,
    pub(super) scroll: u16,
    pub(super) last_selected: Option<usize>,
    pub(super) tree_expanded: bool,
    pub(super) show_urgency_breakdown: bool,
    pub(super) verbose: bool,
    pub(super) show_notes: bool,
}

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
