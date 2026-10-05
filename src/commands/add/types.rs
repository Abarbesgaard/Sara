use crate::infrastructure::model::Task;

#[derive(Default)]
pub struct AddRequest<'a> {
    pub words: &'a [String],
    pub project: Option<&'a str>,
    pub priority: Option<&'a str>,
    pub tags: &'a [String],
    pub recur: Option<&'a str>,
    pub annotations: &'a [String],
    pub links: &'a [String],
    pub checks: &'a [String],
    pub depends_on: &'a [String],
}

pub(super) struct Created {
    pub task: Task,
    pub branch: Option<String>,
    pub similar: anyhow::Result<Vec<serde_json::Value>>,
    pub duplicate: Option<Task>,
}
