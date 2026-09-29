use serde::Deserialize;

use crate::infrastructure::model::RelevantFile;

#[derive(Debug, Deserialize)]
pub(super) struct PlanInput {
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub tasks: Vec<PlanTask>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(super) struct PlanTask {
    /// Local key used to wire dependencies within this plan.
    pub key: Option<String>,
    pub description: String,
    pub assignment: Option<String>,
    pub rationale: Option<String>,
    pub priority: Option<String>,
    pub tags: Vec<String>,
    pub steps: Vec<String>,
    pub acceptance: Vec<String>,
    pub findings: Vec<String>,
    pub constraints: Vec<String>,
    pub files: Vec<RelevantFile>,
    /// Local keys (or existing task ids/uuids) this task depends on.
    pub depends_on: Vec<String>,
}
