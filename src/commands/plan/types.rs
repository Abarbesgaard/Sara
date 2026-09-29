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
    pub depends_on: Vec<String>,
}
