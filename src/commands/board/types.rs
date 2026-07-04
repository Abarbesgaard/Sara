use std::collections::HashSet;

use crate::infrastructure::db::LinkFlags;
use crate::infrastructure::model::Task;

pub enum BoardAction {
    Quit,
    OpenTask(String),
}

/// Tasks tracing back to the same GitHub issue — a collapsible tree node.
/// Collapsed by default so the board opens as a compact list of issues; `o`
/// / `Space` / `Enter` / arrows expand a node to reveal its tasks, neotree-style.
pub struct IssueNode {
    pub owner_repo: String,
    pub number: u64,
    /// Best-effort remote issue title (only known for tasks imported via `sara sync`).
    pub title: Option<String>,
    /// Tasks actually shown as rows — completed ones are pre-filtered out unless
    /// `BoardState::show_finished` is set (see `state::build_state`).
    pub tasks: Vec<Task>,
    /// Completed / total counts across *all* of this issue's tasks, independent
    /// of whether completed ones are currently filtered out of `tasks`.
    pub done: usize,
    pub total: usize,
    pub expanded: bool,
}

pub struct BoardState {
    pub project: String,
    /// Issue-linked tasks, grouped into collapsible nodes (sorted by owner/repo, number).
    /// An issue whose tasks are all completed is omitted entirely when
    /// `show_finished` is false.
    pub issues: Vec<IssueNode>,
    /// Tasks with no linked issue — rendered as flat top-level leaves. Completed
    /// ones are omitted unless `show_finished` is set.
    pub standalone: Vec<Task>,
    /// PR/issue/link flags per task, keyed by uuid string — reuses
    /// `link_flags_by_task`'s already-computed data (not recomputed per row).
    pub badges: std::collections::HashMap<String, LinkFlags>,
    /// Whether completed tasks are included (`sara board --finished`).
    pub show_finished: bool,
    /// Uuids (as strings) of tasks imported by `sara sync` — carry GitHub
    /// provenance, as opposed to tasks that merely link back to an issue for
    /// traceability. Gates the ISS badge so it marks the task that *is* the
    /// synced issue, not every subtask that just traces back to it.
    pub imported: HashSet<String>,
    /// Index into the flattened, expansion-aware row list (see `render::visible_rows`).
    pub selected: usize,
    pub scroll: u16,
    /// Precomputed counts for the title bar — stable between reloads.
    pub pending: usize,
    pub done: usize,
}
