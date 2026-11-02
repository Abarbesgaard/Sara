use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubProvenance {
    pub repo: String,
    #[serde(default)]
    pub issue_id: Option<i64>,
    #[serde(default)]
    pub node_id: Option<String>,
    pub number: i64,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default)]
    pub creator: Option<String>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(alias = "imported_at")]
    pub synced_at: DateTime<Utc>,
    #[serde(alias = "imported_by")]
    pub synced_by: Option<String>,
}
