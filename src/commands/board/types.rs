use crate::infrastructure::model::Task;

pub enum BoardAction {
    Quit,
    OpenTask(String),
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
    /// Precomputed counts for the title bar — stable between reloads.
    pub pending: usize,
    pub done: usize,
    pub feature_count: usize,
    pub selected: usize,
    pub scroll: u16,
}
