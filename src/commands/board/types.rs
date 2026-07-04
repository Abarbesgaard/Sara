use crate::infrastructure::db::LinkFlags;
use crate::infrastructure::model::Task;

pub enum BoardAction {
    Quit,
    OpenTask(String),
    /// Rebuild the board with the other grouping mode ('i' pressed).
    ToggleGrouping,
}

/// How board rows are grouped. Feature (dependency-chain) grouping is the
/// default; issue grouping is opt-in via 'i', reusing `group_tasks_by_issue`
/// (already shipped for `sara list --by-issue`) so a broken-down issue and
/// its tasks read together on the board too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupMode {
    #[default]
    Feature,
    Issue,
}

impl GroupMode {
    pub fn toggled(self) -> Self {
        match self {
            GroupMode::Feature => GroupMode::Issue,
            GroupMode::Issue => GroupMode::Feature,
        }
    }
}

/// A feature = a chain of tasks linked by `sara dep` dependencies (one connected
/// component of the dependency graph). Standalone tasks land in a trailing
/// pseudo-feature with `grouped == false`.
pub struct Feature {
    pub title: String,
    pub done: usize,
    pub total: usize,
    pub grouped: bool,
}

pub struct BoardState {
    pub project: String,
    /// Tasks in feature-grouped, dependency (blockers-first) order.
    pub tasks: Vec<Task>,
    /// Feature index for each task in `tasks`.
    pub feature_of: Vec<usize>,
    pub features: Vec<Feature>,
    /// PR/issue/link flags for each task in `tasks`, same index alignment —
    /// reuses `link_flags_by_task`'s already-computed data (not recomputed).
    pub badges: Vec<LinkFlags>,
    /// Current grouping — preserved across reloads (returning from a task's
    /// detail view) until explicitly toggled with 'i'.
    pub mode: GroupMode,
    /// Precomputed counts for the title bar — stable between reloads.
    pub pending: usize,
    pub done: usize,
    pub feature_count: usize,
    pub selected: usize,
    pub scroll: u16,
}
